//! Debug information (`spec/ll.md` §7.4): the specialized metadata nodes
//! read into `hod_ir`'s debug tables (`spec/ir.md` §6), and the pieces of
//! a body's debug records that are metadata.
//!
//! Two passes over the numbered nodes, in file order, and no recursion
//! through references: the first hands every node an id (a reserved
//! debug node, a reserved location, an interned expression), so that the
//! second can define each node from its text with every reference it
//! makes already resolved. A generic tuple gets an id only when a field
//! names it as a single node; a field that holds a list reads the
//! tuple's elements in place. Nesting happens only for nodes written in
//! place, and that is bounded by `MAX_NESTING`.

use hod_ir::Layering;
use std::borrow::Cow;

use hod_ir::{
    ChecksumKind, ConstId, DiAtom, DiConstant, DiFieldValue, DiFlags, DiId, DiKind, DiLocation,
    DiMapping, DiNode, DiShape, DiString, EmissionKind, ExprId, LocId, NameTableKind, SpFlags,
    di_field, di_kind, dw_ate, dw_cc, dw_lang, dw_op, dw_tag, kit,
};

use crate::Span;
use crate::error::{LlErrorKind, PResult, fail};
use crate::index::parse_u64;
use crate::kw::Kw;
use crate::lexer::Tok;
use crate::meta::{MdKind, MdRef};
use crate::parser::{Parser, ir};
use crate::prelex::Feed;

/// The fields LLVM's parser requires of a node of `kind` (`LLParser`'s
/// `REQUIRED`); `DILocation`'s `scope` is checked where locations are
/// read.
fn required(kind: DiKind) -> &'static [&'static str] {
    match kind {
        DiKind::File => &["filename", "directory"],
        DiKind::Enumerator => &["name", "value"],
        DiKind::DerivedType => &["tag", "baseType"],
        DiKind::CompositeType => &["tag"],
        DiKind::SubroutineType => &["types"],
        DiKind::CompileUnit => &["file"],
        DiKind::LexicalBlock | DiKind::Namespace | DiKind::LocalVariable => &["scope"],
        DiKind::LexicalBlockFile => &["scope", "discriminator"],
        DiKind::TemplateTypeParameter => &["type"],
        DiKind::TemplateValueParameter => &["value"],
        DiKind::GlobalVariableExpression => &["var", "expr"],
        DiKind::ImportedEntity => &["tag", "scope"],
        DiKind::Label => &["scope", "name", "file", "line"],
        _ => &[],
    }
}

/// The kind with a record in `hod_ir` that the metadata index found:
/// `DiKind::named` of the same name, without comparing it again.
fn record_kind(kind: MdKind) -> Option<DiKind> {
    Some(match kind {
        MdKind::GlobalVariableExpression => DiKind::GlobalVariableExpression,
        MdKind::Subrange => DiKind::Subrange,
        MdKind::Enumerator => DiKind::Enumerator,
        MdKind::BasicType => DiKind::BasicType,
        MdKind::DerivedType => DiKind::DerivedType,
        MdKind::CompositeType => DiKind::CompositeType,
        MdKind::SubroutineType => DiKind::SubroutineType,
        MdKind::File => DiKind::File,
        MdKind::CompileUnit => DiKind::CompileUnit,
        MdKind::Subprogram => DiKind::Subprogram,
        MdKind::LexicalBlock => DiKind::LexicalBlock,
        MdKind::LexicalBlockFile => DiKind::LexicalBlockFile,
        MdKind::Namespace => DiKind::Namespace,
        MdKind::TemplateTypeParameter => DiKind::TemplateTypeParameter,
        MdKind::TemplateValueParameter => DiKind::TemplateValueParameter,
        MdKind::GlobalVariable => DiKind::GlobalVariable,
        MdKind::LocalVariable => DiKind::LocalVariable,
        MdKind::Label => DiKind::Label,
        MdKind::ImportedEntity => DiKind::ImportedEntity,
        _ => return None,
    })
}

/// What a numbered metadata node became.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Handle {
    /// Nothing (yet): a tuple no field has named as a node, or metadata
    /// that is not debug information.
    None,
    /// A debug node.
    Node(DiId),
    /// A location.
    Loc(LocId),
    /// An expression.
    Expr(ExprId),
    /// A `DIAssignID`: nothing is kept (§6.9 of `spec/ir.md`).
    AssignId,
}

/// The handles of the numbered nodes, by their position in the
/// metadata index; empty for a module without debug information.
#[derive(Clone, Debug, Default)]
pub(crate) struct DebugMap {
    pub handles: Vec<Handle>,
}

/// The construct recorded when debug metadata breaks a rule LLVM's
/// verifier checks (the kit's).
pub(crate) use hod_ir::kit::DEBUG_INVALID as INVALID;

/// A field's value as read, before the node is built: strings and lists
/// are ranges of scratch storage until then.
#[derive(Clone, Copy, Debug)]
enum Val {
    UInt(u64),
    Int(i64),
    Bool(bool),
    /// A string of the source, or of `strs`.
    Src(u32, u32),
    Buf(u32, u32),
    Node(Option<DiId>),
    /// Elements `lists[start..end]`.
    List(u32, u32),
    Expr(ExprId),
}

/// Which field a value read is for.
#[derive(Clone, Copy, Debug)]
enum Key {
    /// The field at this position of the kind's record
    /// ([`DiKind::fields`]), looked up once.
    At(u8),
    /// One of the older spellings of a subprogram's flags, set by name.
    Name(&'static str),
}

/// The scratch storage of the node being built and of those written in
/// place inside it.
#[derive(Debug, Default)]
pub(crate) struct DebugScratch {
    fields: Vec<(Key, Val)>,
    /// Field names that are not `'static`: a field of the record found by
    /// name in the kind's list.
    lists: Vec<DiId>,
    strs: Vec<u8>,
    elems: Vec<u64>,
    /// Tuples that a field names as a single node: reserved, defined
    /// after the pass.
    tuples: Vec<(DiId, Span)>,
}

/// The name of the node kind written before `span`: the word between the
/// `!` and the `(`.
fn kind_name(src: &[u8], span: Span) -> &[u8] {
    let open = span.start as usize - 1;
    let mut start = open;
    while start > 0 && (src[start - 1].is_ascii_alphanumeric() || src[start - 1] == b'_') {
        start -= 1;
    }
    &src[start..open]
}

/// `DW_VIRTUALITY_*`, the older spelling of a subprogram's virtuality.
fn virtuality(name: &[u8]) -> Option<u16> {
    match name {
        b"DW_VIRTUALITY_none" => Some(0),
        b"DW_VIRTUALITY_virtual" => Some(1),
        b"DW_VIRTUALITY_pure_virtual" => Some(2),
        _ => None,
    }
}

impl<'a, L: Layering> Parser<'a, '_, L> {
    /// The subprogram a function's `!dbg` names, if it has one.
    pub(crate) fn function_dbg(&mut self) -> PResult<Option<DiId>> {
        let mut subprogram = None;
        for at in 0..self.dbg.len() {
            let node = self.dbg[at];
            let node = self.global_dbg(node)?;
            match kit::function_subprogram(self.module, node) {
                Some(id) => subprogram = Some(id),
                None => self.note(INVALID),
            }
        }
        Ok(subprogram)
    }

    /// Attaches the global-variable expressions a variable's `!dbg`
    /// names to `sym`.
    pub(crate) fn variable_dbg(&mut self, sym: hod_ir::SymId) -> PResult<()> {
        for at in 0..self.dbg.len() {
            let node = self.dbg[at];
            let start = self.tok.start;
            let node = self.global_dbg(node)?;
            if !ir(start, kit::variable_expression(self.module, sym, node))? {
                self.note(INVALID);
            }
        }
        Ok(())
    }

    /// The debug node a global value's `!dbg` names: by number, or
    /// written in place. The current token is kept.
    fn global_dbg(&mut self, node: MdRef) -> PResult<Option<DiId>> {
        match node {
            MdRef::Node(id) => match self.handle(id) {
                Handle::Node(id) => Ok(Some(id)),
                _ => Ok(None),
            },
            MdRef::Inline(kind, span) => {
                if matches!(kind, MdKind::Tuple | MdKind::Location | MdKind::Expression) {
                    return Ok(None);
                }
                let resume = self.tok.start;
                let name = kind_name(self.src, span);
                self.goto(span.start - 2 - name.len() as u32);
                let mut scratch = std::mem::take(&mut self.dscratch);
                let made = self.inline_node(&mut scratch);
                let made = made.and_then(|id| self.define_tuples(&mut scratch).map(|()| id));
                scratch.lists.clear();
                self.dscratch = scratch;
                self.goto(resume);
                made.map(Some)
            }
        }
    }

    /// After the items: the module's tables complete, then, when the
    /// module uses them (`!llvm.dbg.cu`, a `!dbg` on a global value),
    /// checked as LLVM's verifier checks them; a failure is
    /// `debug:invalid`, as is debug information LLVM would drop for want
    /// of a `Debug Info Version` of 3.
    pub(crate) fn finish_debug(&mut self) -> PResult<()> {
        ir(self.src.len() as u32, kit::finish_debug(self.module, self.dbg_cu.is_some()))
    }

    /// The handle of the node numbered `id`: at the module level from the
    /// map being built, in a body from the module's text.
    pub(crate) fn handle(&self, id: u32) -> Handle {
        match self.text {
            Some(text) => {
                let md = text.metadata();
                md.position(id).map_or(Handle::None, |at| text.debug_map().handle(at))
            }
            None => self.md.position(id).map_or(Handle::None, |at| self.dmap.handle(at)),
        }
    }

    /// The kind of the node numbered `id`.
    pub(crate) fn md_kind(&self, id: u32) -> Option<MdKind> {
        match self.text {
            Some(text) => text.metadata().node(id).map(|node| node.kind),
            None => self.md.node(id).map(|node| node.kind),
        }
    }

    /// Reads every numbered debug node into the module's tables, then
    /// `!llvm.dbg.cu` (`spec/ll.md` §7.4). Does nothing, and allocates
    /// nothing, when every node is a generic tuple.
    pub(crate) fn build_debug(&mut self) -> PResult<()> {
        self.build_debug_fed(None)
    }

    /// [`Parser::build_debug`], with the tokens of the node loop from
    /// `feed`'s pieces, cut at `cuts` (`spec/ll.md` §4.5) when given.
    pub(crate) fn build_debug_fed(&mut self, feed: Option<(&Feed, &[usize])>) -> PResult<()> {
        let count = self.md.nodes().len();
        let any = self.md.nodes().iter().any(|node| node.kind != MdKind::Tuple);
        if !any && self.dbg_cu.is_none() {
            return Ok(());
        }
        let mut handles = vec![Handle::None; count];
        for (at, handle) in handles.iter_mut().enumerate() {
            let node = self.md.nodes()[at];
            *handle = match node.kind {
                MdKind::Tuple => Handle::None,
                MdKind::Location => {
                    Handle::Loc(ir(node.span.start, self.module.debug.reserve_loc())?)
                }
                MdKind::AssignId => Handle::AssignId,
                MdKind::ArgList => {
                    return fail(
                        LlErrorKind::Syntax,
                        node.span.start,
                        "!DIArgList cannot be a numbered node",
                    );
                }
                MdKind::Expression => Handle::Expr(ExprId::EMPTY),
                _ => Handle::Node(ir(node.span.start, self.module.debug.reserve())?),
            };
        }
        self.dmap.handles = handles;
        let mut scratch = std::mem::take(&mut self.dscratch);
        // Expressions name no other node: read first, so that a node can
        // name one by number.
        for at in 0..count {
            let node = self.md.nodes()[at];
            if node.kind == MdKind::Expression {
                let expr = self.expr_body(node.span, &mut scratch)?;
                self.dmap.handles[at] = Handle::Expr(expr);
            }
        }
        match feed {
            None => {
                for at in 0..count {
                    self.debug_node(at, &mut scratch, false)?;
                }
            }
            Some((feed, cuts)) => self.fed_nodes(feed, cuts, &mut scratch)?,
        }
        if let Some(span) = self.dbg_cu {
            self.goto(span.start);
            while self.tok.start < span.end {
                let at = self.tok.start;
                let unit = self.list_element(&mut scratch)?;
                if !ir(at, kit::compile_unit(self.module, unit))? {
                    self.module.record_unsupported(INVALID, None);
                }
                if self.tok.start < span.end {
                    self.expect(Tok::Comma, "expected ',' between operands")?;
                }
            }
            self.define_tuples(&mut scratch)?;
        }
        scratch.lists.clear();
        self.dscratch = scratch;
        // Equal nodes are one node, as LLVM's parser makes them
        // (`spec/ir.md` §6.3): before anything outside the tables holds
        // an id but the handles.
        let merge = self.module.debug.unique_nodes(hod_ir::DiMark::default());
        if !merge.is_empty() {
            for handle in &mut self.dmap.handles {
                if let Handle::Node(id) = handle {
                    *id = merge.get(*id);
                }
            }
        }
        Ok(())
    }

    /// The node loop with the bodies' tokens from `feed`, a piece at a
    /// time (`spec/ll.md` §4.5): each piece's recording is replayed while
    /// its nodes are read, the lexer moved to each node and tuple with
    /// [`Lexer::seek`](crate::lexer::Lexer::seek), so that a move ahead
    /// to a tuple and back loses none of it.
    #[inline(never)]
    fn fed_nodes(
        &mut self,
        feed: &Feed,
        cuts: &[usize],
        scratch: &mut DebugScratch,
    ) -> PResult<()> {
        for piece in cuts.windows(2) {
            let recording = feed.take(self.lx.record());
            self.lx.replay(recording);
            for at in piece[0]..piece[1] {
                self.lx.seek(self.md.nodes()[at].span.start as usize);
                self.debug_node(at, scratch, true)?;
            }
        }
        let last = self.lx.record();
        self.lx.reuse(last);
        Ok(())
    }

    /// Node `at` of the node loop, then the tuples it named; with `fed`
    /// the lexer is moved with `seek`.
    #[allow(clippy::inline_always)] // The serial loop is this, as before the feed.
    #[inline(always)]
    fn debug_node(&mut self, at: usize, scratch: &mut DebugScratch, fed: bool) -> PResult<()> {
        let node = self.md.nodes()[at];
        match self.dmap.handles[at] {
            // A tuple named as a node is defined by `define_tuples`.
            Handle::Node(_) if node.kind == MdKind::Tuple => {}
            Handle::Node(id) => {
                let made = self.node_body(node.kind, node.span, scratch)?;
                let debug = &mut self.module.debug;
                let result = made.define(debug, id, scratch, self.src, node.distinct);
                ir(node.span.start, result)?;
                scratch.fields.clear();
                scratch.lists.clear();
                scratch.strs.clear();
            }
            Handle::Loc(id) => {
                let (loc, atom, distinct) = match self.quick_location(node.span, scratch)? {
                    Some(loc) => (loc, DiAtom::NONE, node.distinct),
                    None => self.location_body(node.span, node.distinct, scratch)?,
                };
                let debug = &mut self.module.debug;
                let result = debug.define_loc_in_atom(id, loc, atom, distinct);
                ir(node.span.start, result)?;
            }
            Handle::None | Handle::Expr(_) | Handle::AssignId => {}
        }
        self.define_tuples_in(scratch, fed)
    }

    /// Defines the tuples fields named as single nodes, and those their
    /// elements named in turn.
    fn define_tuples(&mut self, scratch: &mut DebugScratch) -> PResult<()> {
        self.define_tuples_in(scratch, false)
    }

    /// [`Parser::define_tuples`]; with `fed` the lexer is moved with
    /// `seek`.
    fn define_tuples_in(&mut self, scratch: &mut DebugScratch, fed: bool) -> PResult<()> {
        while let Some((id, span)) = scratch.tuples.pop() {
            let mark = scratch.lists.len();
            if fed {
                self.lx.seek(span.start as usize);
                self.bump();
            } else {
                self.goto(span.start);
            }
            while self.tok.start < span.end {
                let element = self.list_element(scratch)?.unwrap_or(DiId::NONE);
                scratch.lists.push(element);
                if self.tok.start < span.end {
                    self.expect(Tok::Comma, "expected ',' between operands")?;
                }
            }
            let elements = &scratch.lists[mark..];
            ir(span.start, kit::define_di_tuple(&mut self.module.debug, id, elements))?;
            scratch.lists.truncate(mark);
        }
        Ok(())
    }

    // ---- references ----

    /// The single node a field names: `!N`, `null`, `!"…"`, `!{…}` or a
    /// node written in place, or a constant as metadata (`i32 7`).
    /// `None` for `null`.
    fn node_ref(&mut self, scratch: &mut DebugScratch) -> PResult<Option<DiId>> {
        let at = self.tok.start;
        match self.tok.kind {
            Tok::Word if self.text() == b"null" => {
                self.bump();
                Ok(None)
            }
            Tok::Exclaim => {
                self.bump();
                match self.tok.kind {
                    Tok::Str => {
                        let text = self.string(self.tok);
                        self.bump();
                        let node = DiNode::String(DiString { text: &text });
                        Ok(Some(ir(at, self.module.debug.add(&node))?))
                    }
                    Tok::LBrace => {
                        let span = self.skip_brackets()?;
                        let id = ir(at, self.module.debug.reserve())?;
                        scratch.tuples.push((id, span));
                        Ok(Some(id))
                    }
                    Tok::Int => {
                        let number = self.tok;
                        self.bump();
                        let Some(id) = crate::index::parse_u32(self.tok_text(number)) else {
                            return fail(
                                LlErrorKind::Syntax,
                                at,
                                "expected a metadata node number",
                            );
                        };
                        self.numbered_node(at, id, scratch).map(Some)
                    }
                    _ => self.syntax("expected a metadata node"),
                }
            }
            Tok::Meta => self.inline_node(scratch).map(Some),
            _ => {
                let value = self.parse_typed_const()?;
                Ok(Some(self.constant_node(at, value)?))
            }
        }
    }

    /// `!N` where a node is expected.
    fn numbered_node(&mut self, at: u32, id: u32, scratch: &mut DebugScratch) -> PResult<DiId> {
        let Some(position) = self.md_position(id) else {
            return fail(LlErrorKind::Undefined, at, format!("use of undefined metadata '!{id}'"));
        };
        match self.handle_at(position) {
            Handle::Node(node) => Ok(node),
            Handle::None if self.text.is_none() => {
                // A tuple named as a node.
                let span = self.md.nodes()[position].span;
                let node = ir(at, self.module.debug.reserve())?;
                self.dmap.handles[position] = Handle::Node(node);
                scratch.tuples.push((node, span));
                Ok(node)
            }
            Handle::None | Handle::Loc(_) | Handle::Expr(_) | Handle::AssignId => {
                // Not a node a field may name; LLVM's verifier refuses it.
                self.invalid();
                ir(at, kit::invalid_di_ref(&mut self.module.debug))
            }
        }
    }

    fn constant_node(&mut self, at: u32, value: ConstId) -> PResult<DiId> {
        let node = DiNode::Constant(DiConstant { value });
        ir(at, self.module.debug.add(&node))
    }

    /// One element of a tuple read as a list: `None` for `null`.
    fn list_element(&mut self, scratch: &mut DebugScratch) -> PResult<Option<DiId>> {
        self.node_ref(scratch)
    }

    /// The elements of the list a field names: a tuple by number or in
    /// place, or `null`. Pushed to `scratch.lists`; their range.
    fn list_ref(&mut self, scratch: &mut DebugScratch) -> PResult<(u32, u32)> {
        let mark = scratch.lists.len() as u32;
        let at = self.tok.start;
        let span = match self.tok.kind {
            Tok::Word if self.text() == b"null" => {
                self.bump();
                return Ok((mark, mark));
            }
            Tok::Exclaim => {
                self.bump();
                match self.tok.kind {
                    Tok::LBrace => {
                        let span = self.skip_brackets()?;
                        let resume = self.tok.start;
                        self.list_span(span, scratch)?;
                        self.goto(resume);
                        return Ok((mark, scratch.lists.len() as u32));
                    }
                    Tok::Int => {
                        let number = self.tok;
                        self.bump();
                        let Some(id) = crate::index::parse_u32(self.tok_text(number)) else {
                            return fail(
                                LlErrorKind::Syntax,
                                at,
                                "expected a metadata node number",
                            );
                        };
                        let Some(position) = self.md.position(id) else {
                            let message = format!("use of undefined metadata '!{id}'");
                            return fail(LlErrorKind::Undefined, at, message);
                        };
                        let node = self.md.nodes()[position];
                        if node.kind != MdKind::Tuple {
                            // LLVM's verifier wants a tuple here.
                            self.module.record_unsupported(INVALID, None);
                            return Ok((mark, mark));
                        }
                        node.span
                    }
                    _ => return self.syntax("expected a metadata tuple"),
                }
            }
            _ => {
                self.module.record_unsupported(INVALID, None);
                self.skip_value()?;
                return Ok((mark, mark));
            }
        };
        let resume = self.tok.start;
        self.list_span(span, scratch)?;
        self.goto(resume);
        Ok((mark, scratch.lists.len() as u32))
    }

    /// Pushes the elements of the tuple whose inside is `span`.
    fn list_span(&mut self, span: Span, scratch: &mut DebugScratch) -> PResult<()> {
        self.goto(span.start);
        while self.tok.start < span.end {
            let element = self.list_element(scratch)?.unwrap_or(DiId::NONE);
            scratch.lists.push(element);
            if self.tok.start < span.end {
                self.expect(Tok::Comma, "expected ',' between operands")?;
            }
        }
        Ok(())
    }

    /// An expression a field names: in place, or a numbered one.
    pub(crate) fn expr_ref(&mut self, scratch: &mut DebugScratch) -> PResult<ExprId> {
        let at = self.tok.start;
        match self.tok.kind {
            Tok::Meta if self.text() == b"!DIExpression" => {
                self.bump();
                if self.tok.kind != Tok::LParen {
                    return self.syntax("expected '(' here");
                }
                let span = self.skip_brackets()?;
                let resume = self.tok.start;
                let expr = self.expr_body(span, scratch)?;
                self.goto(resume);
                Ok(expr)
            }
            Tok::Exclaim => {
                self.bump();
                let number = self.expect(Tok::Int, "expected a metadata node")?;
                let Some(id) = crate::index::parse_u32(self.tok_text(number)) else {
                    return fail(LlErrorKind::Syntax, at, "expected a metadata node number");
                };
                match self.md_position(id).map(|at| self.handle_at(at)) {
                    Some(Handle::Expr(expr)) => Ok(expr),
                    Some(_) => fail(LlErrorKind::Syntax, at, "expected a DIExpression"),
                    None => fail(
                        LlErrorKind::Undefined,
                        at,
                        format!("use of undefined metadata '!{id}'"),
                    ),
                }
            }
            _ => self.syntax("expected a DIExpression"),
        }
    }

    fn md_position(&self, id: u32) -> Option<usize> {
        match self.text {
            Some(text) => text.metadata().position(id),
            None => self.md.position(id),
        }
    }

    fn handle_at(&self, at: usize) -> Handle {
        match self.text {
            Some(text) => text.debug_map().handle(at),
            None => self.dmap.handle(at),
        }
    }

    /// The inside of `!DIExpression(…)`, interned.
    pub(crate) fn expr_body(&mut self, span: Span, scratch: &mut DebugScratch) -> PResult<ExprId> {
        scratch.elems.clear();
        self.goto(span.start);
        while self.tok.start < span.end {
            let at = self.tok.start;
            let text = self.text();
            let element = match self.tok.kind {
                Tok::Int => parse_u64(text),
                Tok::Word if text.starts_with(b"DW_OP_") => dw_op(text),
                Tok::Word if text.starts_with(b"DW_ATE_") => dw_ate(text).map(u64::from),
                _ => return self.syntax("expected a DWARF operation or a number"),
            };
            let Some(element) = element else {
                let what = String::from_utf8_lossy(text);
                return fail(
                    LlErrorKind::Syntax,
                    at,
                    format!("invalid element '{what}' of a DIExpression"),
                );
            };
            scratch.elems.push(element);
            self.bump();
            if self.tok.start < span.end {
                self.expect(Tok::Comma, "expected ',' between operands")?;
            }
        }
        ir(span.start, self.module.debug.expr_from_elements(&scratch.elems))
    }

    // ---- nodes ----

    /// A node written in place, at its `!DIKind`: added (uniqued).
    fn inline_node(&mut self, scratch: &mut DebugScratch) -> PResult<DiId> {
        let at = self.tok.start;
        let name = &self.text()[1..];
        let kind = MdKind::from_name(name);
        self.bump();
        if self.tok.kind != Tok::LParen {
            return self.syntax("expected '(' here");
        }
        let span = self.skip_brackets()?;
        let resume = self.tok.start;
        if matches!(
            kind,
            MdKind::Location | MdKind::Expression | MdKind::ArgList | MdKind::AssignId
        ) {
            // Not a node a field may name.
            self.module.record_unsupported(INVALID, None);
            return ir(at, kit::invalid_di_ref(&mut self.module.debug));
        }
        self.enter()?;
        let fields = scratch.fields.len();
        let lists = scratch.lists.len();
        let strs = scratch.strs.len();
        let src = self.src;
        let made = self.node_body(kind, span, scratch)?;
        let id = made.add(&mut self.module.debug, &scratch.fields[fields..], scratch, src);
        let id = ir(at, id)?;
        scratch.fields.truncate(fields);
        scratch.lists.truncate(lists);
        scratch.strs.truncate(strs);
        self.leave();
        self.goto(resume);
        Ok(id)
    }

    /// Reads the fields of a node of the kind `name` from the inside of
    /// its parentheses, `span`, into `scratch.fields`.
    fn node_body(
        &mut self,
        md: MdKind,
        span: Span,
        scratch: &mut DebugScratch,
    ) -> PResult<Made<'a>> {
        // A kind with a record is mapped (`di_kind`), and the index has
        // named it already; the text and the table are asked only about
        // the others.
        let Some(kind) = record_kind(md) else {
            let name = kind_name(self.src, span);
            if di_kind(name).is_none() {
                let at = span.start - 2 - name.len() as u32;
                return fail(LlErrorKind::Syntax, at, "expected metadata type");
            }
            return Ok(Made { kind: None, name, first: scratch.fields.len() });
        };
        let name = kind.llvm_name().as_bytes();
        let first = scratch.fields.len();
        // The fields met, for LLVM's two rules: none twice, and the
        // required ones present. The record's fields by position (at most
        // 32, `spec/ir.md` §6.2), the few others by name.
        let mut met = 0u64;
        let mut seen: [&[u8]; 48] = [b""; 48];
        let mut count = 0;
        // Where the next field is looked for first: LLVM writes them in
        // the record's order.
        let mut hint = 0;
        // The fields LLVM's writer prints are read from the bytes
        // (`quick_field`) while they come in its spelling; the first one
        // that does not, and every one after it, through the tokens.
        let end = span.end as usize;
        let mut pos = span.start as usize;
        loop {
            if pos >= end {
                break;
            }
            match self.quick_field(kind, pos, end, hint, met, scratch)? {
                Some((index, value, next)) => {
                    met |= 1 << index;
                    hint = index + 1;
                    scratch.fields.push((Key::At(index as u8), value));
                    pos = next;
                }
                None => {
                    self.goto(pos as u32);
                    break;
                }
            }
        }
        while pos < end && self.tok.start < span.end {
            let (field, at) = self.field_name()?;
            let index = kind.field_index(field, hint);
            let again = match index {
                Some(index) => met & (1 << index) != 0,
                None => seen[..count].contains(&field),
            };
            if again {
                let field = String::from_utf8_lossy(field);
                let message = format!("field '{field}' cannot be specified more than once");
                return fail(LlErrorKind::Syntax, at, message);
            }
            if let Some(index) = index {
                met |= 1 << index;
                hint = index + 1;
                let shape = kind.field_shape_at(index);
                let value = self.field_value(kind, shape, field, scratch)?;
                scratch.fields.push((Key::At(index as u8), value));
                if self.tok.start < span.end {
                    self.expect(Tok::Comma, "expected ',' between fields")?;
                }
                continue;
            }
            if count < seen.len() {
                seen[count] = field;
                count += 1;
            }
            match di_field(name, field) {
                Some(DiMapping::Mapped) => {
                    // Not in the record: an older spelling, no shape.
                    let value = self.field_value(kind, None, field, scratch)?;
                    scratch.fields.push((Key::Name(older_spelling(field)), value));
                }
                Some(DiMapping::Dropped(_)) => {
                    self.skip_value()?;
                }
                Some(DiMapping::Unsupported(_)) => {
                    let default = self.skip_value()?;
                    kit::unsupported_field(self.module, name, field, default);
                }
                None => {
                    let field = String::from_utf8_lossy(field);
                    return fail(LlErrorKind::Syntax, at, format!("invalid field '{field}'"));
                }
            }
            if self.tok.start < span.end {
                self.expect(Tok::Comma, "expected ',' between fields")?;
            }
        }
        for &field in required(kind) {
            let present = match kind.field_index(field.as_bytes(), 0) {
                Some(index) => met & (1 << index) != 0,
                None => seen[..count].contains(&field.as_bytes()),
            };
            if !present {
                let message = format!("missing required field '{field}'");
                return fail(LlErrorKind::Syntax, span.end, message);
            }
        }
        Ok(Made { kind: Some(kind), name, first })
    }

    /// `name:` at the start of a field: the name and where it is.
    fn field_name(&mut self) -> PResult<(&'a [u8], u32)> {
        let token = self.tok;
        let text = self.tok_text(token);
        match token.kind {
            Tok::Label => {
                self.bump();
                Ok((&text[..text.len() - 1], token.start))
            }
            Tok::Word => {
                self.bump();
                self.expect(Tok::Colon, "expected ':' after a field name")?;
                Ok((text, token.start))
            }
            _ => self.syntax("expected a field name"),
        }
    }

    /// One field of a node of `kind` read from the bytes at `pos`, without
    /// the lexer (WP-328): `name: value` as LLVM's writer prints it,
    /// followed by `,` or by the node's `)` at `end`. The field, its value
    /// and where the next field starts; `None`, before anything is read
    /// into the module or the scratch storage, for any other spelling,
    /// for a field outside the record or met before, and for a value
    /// whose reading the token path must judge: that path then reads the
    /// field from `pos`, with its diagnostics.
    fn quick_field(
        &mut self,
        kind: DiKind,
        pos: usize,
        end: usize,
        hint: usize,
        met: u64,
        scratch: &mut DebugScratch,
    ) -> PResult<Option<(usize, Val, usize)>> {
        let src = self.src;
        let name_end = word_end(src, pos, end);
        if name_end == pos
            || !src[pos].is_ascii_alphabetic()
            || src.get(name_end..name_end + 2) != Some(b": ")
        {
            return Ok(None);
        }
        let field = &src[pos..name_end];
        let Some(index) = kind.field_index(field, hint) else {
            return Ok(None);
        };
        if met & (1 << index) != 0 {
            return Ok(None);
        }
        let shape = kind.field_shape_at(index);
        // The value: its form, and the byte after it.
        let at = name_end + 2;
        let (form, after) = match shape {
            Some(DiShape::Flags | DiShape::SpFlags) => {
                let Some((bits, after)) = quick_flags(src, at, end, shape == Some(DiShape::Flags))
                else {
                    return Ok(None);
                };
                (Quick::Bits(bits), after)
            }
            Some(DiShape::Expr | DiShape::Bound) => return Ok(None),
            _ => match src.get(at) {
                Some(b'"') => {
                    let inner = &src[at + 1..end];
                    let Some(close) = inner.iter().position(|&b| b == b'"' || b == b'\\') else {
                        return Ok(None);
                    };
                    if inner[close] != b'"' {
                        return Ok(None);
                    }
                    (Quick::Str(at + 1, at + 1 + close), at + close + 2)
                }
                Some(b'!') => {
                    let digits = digits_end(src, at + 1, end);
                    if digits == at + 1 {
                        return Ok(None);
                    }
                    (Quick::Ref(at + 1, digits), digits)
                }
                Some(b'-' | b'0'..=b'9') => {
                    let first = if src[at] == b'-' { at + 1 } else { at };
                    let digits = digits_end(src, first, end);
                    if digits == first {
                        return Ok(None);
                    }
                    (Quick::Int(at, digits), digits)
                }
                Some(&b) if b.is_ascii_alphabetic() || b == b'_' => {
                    let word = word_end(src, at, end);
                    (Quick::Word(at, word), word)
                }
                _ => return Ok(None),
            },
        };
        let next = if after == end {
            end
        } else if src.get(after..after + 2) == Some(b", ") && after + 2 < end {
            // `, ` and the next field: a trailing comma, another blank
            // or a comment is the token path's to judge.
            after + 2
        } else {
            return Ok(None);
        };
        // As `field_value` reads the same tokens; what it would refuse,
        // record or read in another way is left to it.
        let value = match (shape, form) {
            (_, Quick::Bits(bits)) => Val::UInt(bits),
            (Some(DiShape::List), Quick::Word(start, word)) if &src[start..word] == b"null" => {
                let mark = scratch.lists.len() as u32;
                Val::List(mark, mark)
            }
            (Some(DiShape::List), Quick::Ref(start, digits)) => {
                let Some(id) = crate::index::parse_u32(&src[start..digits]) else {
                    return Ok(None);
                };
                let Some(position) = self.md.position(id) else {
                    return Ok(None);
                };
                let node = self.md.nodes()[position];
                if node.kind != MdKind::Tuple {
                    return Ok(None);
                }
                let mark = scratch.lists.len() as u32;
                self.list_span(node.span, scratch)?;
                Val::List(mark, scratch.lists.len() as u32)
            }
            (Some(DiShape::List), _) => return Ok(None),
            (_, Quick::Str(start, close)) => Val::Src(start as u32, close as u32),
            (Some(DiShape::Node), Quick::Int(..)) => return Ok(None),
            (_, Quick::Int(start, digits)) => {
                let text = &src[start..digits];
                if let Some(negative) = text.strip_prefix(b"-") {
                    match parse_u64(negative) {
                        Some(magnitude) if magnitude <= 1 << 63 => {
                            Val::Int((magnitude as i64).wrapping_neg())
                        }
                        _ => return Ok(None),
                    }
                } else {
                    match parse_u64(text) {
                        Some(value) => Val::UInt(value),
                        None => return Ok(None),
                    }
                }
            }
            (_, Quick::Word(start, word)) => match &src[start..word] {
                b"true" => Val::Bool(true),
                b"false" => Val::Bool(false),
                b"null" => Val::Node(None),
                text => match named_code(shape, field, text) {
                    Some(code) => Val::UInt(code),
                    None => return Ok(None),
                },
            },
            (_, Quick::Ref(start, digits)) => {
                let Some(id) = crate::index::parse_u32(&src[start..digits]) else {
                    return Ok(None);
                };
                Val::Node(Some(self.numbered_node(at as u32, id, scratch)?))
            }
        };
        Ok(Some((index, value, next)))
    }

    /// `!DILocation(line: N, column: N, scope: !N[, inlinedAt: !N])` read
    /// from the bytes of its inside, `span`, without the lexer (WP-328).
    /// `None`, before anything is read into the module, for any other
    /// spelling or a value [`Parser::location_body`] must judge.
    fn quick_location(
        &mut self,
        span: Span,
        scratch: &mut DebugScratch,
    ) -> PResult<Option<DiLocation>> {
        let src = self.src;
        let (mut pos, end) = (span.start as usize, span.end as usize);
        let mut line = None;
        let mut column = None;
        let mut scope = None;
        let mut inlined = None;
        while pos < end {
            let name_end = word_end(src, pos, end);
            if src.get(name_end..name_end + 2) != Some(b": ") {
                return Ok(None);
            }
            let at = name_end + 2;
            let reference = src.get(at) == Some(&b'!');
            let first = at + usize::from(reference);
            let digits = digits_end(src, first, end);
            let Some(value) = crate::index::parse_u32(&src[first..digits]) else {
                return Ok(None);
            };
            let slot = match (&src[pos..name_end], reference) {
                (b"line", false) => &mut line,
                (b"column", false) if value <= u32::from(u16::MAX) => &mut column,
                (b"scope", true) => &mut scope,
                (b"inlinedAt", true) if scope.is_some() && self.has_md_node(value) => &mut inlined,
                _ => return Ok(None),
            };
            if slot.is_some() {
                return Ok(None);
            }
            *slot = Some((first as u32 - 1, value));
            pos = if digits == end {
                end
            } else if src.get(digits..digits + 2) == Some(b", ") && digits + 2 < end {
                digits + 2
            } else {
                return Ok(None);
            };
        }
        let Some((at, scope)) = scope else {
            return Ok(None);
        };
        let scope = self.numbered_node(at, scope, scratch)?;
        let inlined_at = match inlined {
            Some((at, id)) => self.loc_of(at, MdRef::Node(id))?.unwrap_or(LocId::NONE),
            None => LocId::NONE,
        };
        Ok(Some(DiLocation {
            line: line.map_or(0, |(_, line)| line),
            column: column.map_or(0, |(_, column)| column as u16),
            scope,
            inlined_at,
        }))
    }

    /// Steps over a field's value. True if it is a default (`0`,
    /// `false`, `null`, `""`, `!{}`).
    fn skip_value(&mut self) -> PResult<bool> {
        let start = self.tok;
        let mut tokens = 0;
        let mut angle = 0u32;
        loop {
            match self.tok.kind {
                Tok::Comma | Tok::RParen if angle == 0 => break,
                Tok::Eof | Tok::Error => return self.syntax("expected a field value"),
                Tok::LParen | Tok::LBrace | Tok::LBracket => {
                    self.skip_brackets()?;
                }
                Tok::Less => {
                    angle += 1;
                    self.bump();
                }
                Tok::Greater => {
                    angle = angle.saturating_sub(1);
                    self.bump();
                }
                _ => self.bump(),
            }
            tokens += 1;
        }
        let text = self.tok_text(start);
        let default = match tokens {
            1 => matches!(text, b"0" | b"false" | b"null" | b"\"\""),
            2 => {
                text == b"!"
                    && self.src.get(start.end as usize..start.end as usize + 2) == Some(b"{}")
            }
            _ => false,
        };
        Ok(default)
    }

    /// A field's value, read by what the field holds (`shape`, none for a
    /// field that is not in the kind's record).
    fn field_value(
        &mut self,
        kind: DiKind,
        shape: Option<DiShape>,
        field: &[u8],
        scratch: &mut DebugScratch,
    ) -> PResult<Val> {
        let at = self.tok.start;
        let token = self.tok;
        let text = self.text();
        match shape {
            Some(DiShape::List) => {
                let (start, end) = self.list_ref(scratch)?;
                return Ok(Val::List(start, end));
            }
            Some(DiShape::Expr) => return Ok(Val::Expr(self.expr_ref(scratch)?)),
            Some(DiShape::Flags | DiShape::SpFlags) => {
                return Ok(Val::UInt(self.flag_bits(shape == Some(DiShape::Flags))?));
            }
            _ => {}
        }
        // A word's code, looked up once (the `true`, `false` and `null`
        // arms come first and never consult it).
        let code = match token.kind {
            Tok::Word if !matches!(text, b"true" | b"false" | b"null") => {
                named_code(shape, field, text)
            }
            _ => None,
        };
        let value = match token.kind {
            Tok::Str => {
                let string = self.string(token);
                self.bump();
                match string {
                    Cow::Borrowed(_) => Val::Src(token.start + 1, token.end - 1),
                    Cow::Owned(bytes) => {
                        let start = scratch.strs.len() as u32;
                        scratch.strs.extend_from_slice(&bytes);
                        Val::Buf(start, scratch.strs.len() as u32)
                    }
                }
            }
            Tok::Int if !matches!(shape, Some(DiShape::Node)) => {
                self.bump();
                if let Some(negative) = text.strip_prefix(b"-") {
                    match parse_u64(negative) {
                        Some(magnitude) if magnitude <= 1 << 63 => {
                            Val::Int((magnitude as i64).wrapping_neg())
                        }
                        _ => return self.wide(kind, at),
                    }
                } else {
                    match parse_u64(text) {
                        Some(value) => Val::UInt(value),
                        None => return self.wide(kind, at),
                    }
                }
            }
            Tok::Word if text == b"true" || text == b"false" => {
                self.bump();
                Val::Bool(text == b"true")
            }
            Tok::Word if code.is_some() => {
                self.bump();
                Val::UInt(code.unwrap_or(0))
            }
            Tok::Word
                if !matches!(shape, Some(DiShape::Node | DiShape::Bound))
                    && text != b"null"
                    && (text.starts_with(b"DW_") || text.starts_with(b"CSK_")) =>
            {
                let what = String::from_utf8_lossy(text);
                return fail(LlErrorKind::Syntax, at, format!("invalid value '{what}' of a field"));
            }
            Tok::Meta if text == b"!DIExpression" => Val::Expr(self.expr_ref(scratch)?),
            Tok::Exclaim if shape == Some(DiShape::Bound) => {
                // A bound by number may name an expression.
                self.bump();
                let number = self.tok;
                let expr = match number.kind {
                    Tok::Int => crate::index::parse_u32(self.tok_text(number))
                        .and_then(|id| self.md_position(id))
                        .map(|position| self.handle_at(position)),
                    _ => None,
                };
                if let Some(Handle::Expr(expr)) = expr {
                    self.bump();
                    Val::Expr(expr)
                } else {
                    self.goto(at);
                    Val::Node(self.node_ref(scratch)?)
                }
            }
            _ => Val::Node(self.node_ref(scratch)?),
        };
        Ok(value)
    }

    /// `A | B | 7`: a set of `DIFlag*` (`di`) or `DISPFlag*` names.
    fn flag_bits(&mut self, di: bool) -> PResult<u64> {
        let mut bits = 0u64;
        loop {
            let at = self.tok.start;
            let text = self.text();
            let flag = match self.tok.kind {
                Tok::Int => parse_u64(text),
                Tok::Word if di => DiFlags::named(text).map(|flags| u64::from(flags.0)),
                Tok::Word => SpFlags::named(text).map(|flags| u64::from(flags.0)),
                _ => None,
            };
            let Some(flag) = flag else {
                let what = String::from_utf8_lossy(text);
                return fail(LlErrorKind::Syntax, at, format!("invalid flag '{what}'"));
            };
            bits |= flag;
            self.bump();
            if !self.eat(Tok::Bar) {
                return Ok(bits);
            }
        }
    }

    /// An integer that does not fit 64 bits: a wide enumerator.
    fn wide(&mut self, kind: DiKind, at: u32) -> PResult<Val> {
        if kind == DiKind::Enumerator {
            self.module.record_unsupported(kit::WIDE_ENUMERATOR, None);
            return Ok(Val::UInt(0));
        }
        fail(LlErrorKind::Syntax, at, "a number of a debug node does not fit 64 bits")
    }

    /// The inside of `!DILocation(…)`: the location, its key-instruction
    /// atom (WP-149), and whether it is distinct.
    fn location_body(
        &mut self,
        span: Span,
        distinct: bool,
        scratch: &mut DebugScratch,
    ) -> PResult<(DiLocation, DiAtom, bool)> {
        let mut loc = DiLocation { line: 0, column: 0, scope: DiId::NONE, inlined_at: LocId::NONE };
        let mut atom = DiAtom::NONE;
        let mut scope = false;
        self.goto(span.start);
        while self.tok.start < span.end {
            let (field, at) = self.field_name()?;
            match field {
                b"line" => loc.line = self.expect_u32("expected a line number")?,
                b"column" => {
                    let column = self.expect_u32("expected a column number")?;
                    let Ok(column) = u16::try_from(column) else {
                        return fail(
                            LlErrorKind::Syntax,
                            at,
                            "value for 'column' too large, limit is 65535",
                        );
                    };
                    loc.column = column;
                }
                b"scope" => {
                    scope = true;
                    match self.node_ref(scratch)? {
                        Some(node) => loc.scope = node,
                        None => {
                            return fail(
                                LlErrorKind::Syntax,
                                at,
                                "a DILocation's scope is not null",
                            );
                        }
                    }
                }
                b"inlinedAt" => loc.inlined_at = self.loc_ref(scratch)?.unwrap_or(LocId::NONE),
                // WP-149: LLVM's limits are 64 and 8 bits; what it does not
                // store (61 and 3 bits) `hod_ir` refuses.
                b"atomGroup" => atom.group = self.expect_u64("expected an atom group")?,
                b"atomRank" => {
                    let rank = self.expect_u64("expected an atom rank")?;
                    let Some(read) = kit::di_atom(atom.group, rank) else {
                        return fail(
                            LlErrorKind::Syntax,
                            at,
                            "value for 'atomRank' too large, limit is 255",
                        );
                    };
                    atom = read;
                }
                _ => {
                    if let Some(DiMapping::Dropped(_)) = di_field(b"DILocation", field) {
                        self.skip_value()?;
                    } else {
                        let field = String::from_utf8_lossy(field);
                        return fail(LlErrorKind::Syntax, at, format!("invalid field '{field}'"));
                    }
                }
            }
            if self.tok.start < span.end {
                self.expect(Tok::Comma, "expected ',' between fields")?;
            }
        }
        if !scope {
            return fail(LlErrorKind::Syntax, span.end, "missing required field 'scope'");
        }
        Ok((loc, atom, distinct))
    }

    /// A location by number, or `null`: `None` for `null`.
    fn loc_ref(&mut self, scratch: &mut DebugScratch) -> PResult<Option<LocId>> {
        let at = self.tok.start;
        if self.eat_word(Kw::Null) {
            return Ok(None);
        }
        let node = self.md_ref()?;
        let resume = self.tok.start;
        let loc = match node {
            MdRef::Inline(MdKind::Location, span) => {
                self.enter()?;
                let (loc, atom, distinct) = self.location_body(span, false, scratch)?;
                self.leave();
                Some(ir(at, kit::add_location(&mut self.module.debug, loc, atom, distinct))?)
            }
            node => self.loc_of(at, node)?,
        };
        self.goto(resume);
        Ok(loc)
    }

    /// The location `node` is; `None` (and `debug:invalid`) for a node
    /// that is no `DILocation`. Moves the lexer for one in place.
    pub(crate) fn loc_of(&mut self, at: u32, node: MdRef) -> PResult<Option<LocId>> {
        match node {
            MdRef::Node(id) => match self.md_position(id).map(|at| self.handle_at(at)) {
                Some(Handle::Loc(loc)) => Ok(Some(loc)),
                _ => Ok(self.invalid_loc()),
            },
            MdRef::Inline(MdKind::Location, span) => {
                let mut scratch = std::mem::take(&mut self.dscratch);
                let read = self.location_body(span, false, &mut scratch);
                let read = read.and_then(|read| self.define_tuples(&mut scratch).map(|()| read));
                scratch.lists.clear();
                self.dscratch = scratch;
                let (loc, atom, distinct) = read?;
                Ok(Some(ir(at, kit::add_location(&mut self.module.debug, loc, atom, distinct))?))
            }
            MdRef::Inline(..) => Ok(self.invalid_loc()),
        }
    }

    /// A location that names no `DILocation`: LLVM's verifier refuses it.
    fn invalid_loc(&mut self) -> Option<LocId> {
        self.invalid();
        None
    }

    /// Records `debug:invalid`: against the function in a body, the
    /// module otherwise.
    fn invalid(&mut self) {
        if self.text.is_some() {
            self.note(INVALID);
        } else {
            self.module.record_unsupported(INVALID, None);
        }
    }
}

/// The number a named value of `field` stands for: an enumeration's
/// keyword, or a DWARF constant by name.
fn named_code(shape: Option<DiShape>, field: &[u8], text: &[u8]) -> Option<u64> {
    let code = match shape {
        Some(DiShape::EmissionKind) => EmissionKind::named(text).map(EmissionKind::code),
        Some(DiShape::NameTableKind) => NameTableKind::named(text).map(NameTableKind::code),
        Some(DiShape::ChecksumKind) => ChecksumKind::named(text).map(ChecksumKind::code),
        _ => None,
    };
    if let Some(code) = code {
        return Some(u64::from(code));
    }
    let code = match field {
        b"tag" => dw_tag(text),
        b"encoding" => dw_ate(text),
        b"language" | b"runtimeLang" => dw_lang(text),
        b"cc" => dw_cc(text),
        b"virtuality" => virtuality(text),
        _ => None,
    };
    code.map(u64::from)
}

/// The form of a value `quick_field` read: byte offsets into the source.
#[derive(Clone, Copy)]
enum Quick {
    /// `"…"` without an escape: the inside.
    Str(usize, usize),
    /// `!N`: the digits.
    Ref(usize, usize),
    /// `N` or `-N`.
    Int(usize, usize),
    /// A word.
    Word(usize, usize),
    /// A set of flags, read.
    Bits(u64),
}

/// The end of the word (`[A-Za-z0-9_]*`) at `from`, at most `end`.
fn word_end(src: &[u8], from: usize, end: usize) -> usize {
    let mut at = from;
    while at < end && (src[at].is_ascii_alphanumeric() || src[at] == b'_') {
        at += 1;
    }
    at
}

/// The end of the decimal digits at `from`, at most `end`.
fn digits_end(src: &[u8], from: usize, end: usize) -> usize {
    let mut at = from;
    while at < end && src[at].is_ascii_digit() {
        at += 1;
    }
    at
}

/// `A | B | 7` at `at` read from the bytes, as `Parser::flag_bits` reads
/// the tokens: `DIFlag*` names (`di`) or `DISPFlag*` names, and numbers,
/// joined by ` | `. The bits and the byte after them; `None` for anything
/// else (that parser then judges it).
fn quick_flags(src: &[u8], mut at: usize, end: usize, di: bool) -> Option<(u64, usize)> {
    let mut bits = 0u64;
    loop {
        let word = word_end(src, at, end);
        let text = src.get(at..word).filter(|text| !text.is_empty())?;
        bits |= if text[0].is_ascii_digit() {
            parse_u64(text)?
        } else if di {
            u64::from(DiFlags::named(text)?.0)
        } else {
            u64::from(SpFlags::named(text)?.0)
        };
        if src.get(word..word + 3) == Some(b" | ") && word + 3 < end {
            at = word + 3;
        } else {
            return Some((bits, word));
        }
    }
}

/// The `'static` name of a mapped field LLVM calls `field` that is not
/// in the kind's record: one of the older spellings a subprogram takes.
fn older_spelling(field: &[u8]) -> &'static str {
    ["isLocal", "isDefinition", "isOptimized", "virtuality"]
        .into_iter()
        .find(|known| known.as_bytes() == field)
        .unwrap_or("")
}

/// What `node_body` read: the kind (none for a kind `hod_ir` does not
/// represent) and where its fields start in the scratch storage.
pub(crate) struct Made<'a> {
    kind: Option<DiKind>,
    name: &'a [u8],
    first: usize,
}

impl Made<'_> {
    /// The node, its fields set from `fields`.
    fn node<'s>(
        &'s self,
        fields: &'s [(Key, Val)],
        scratch: &'s DebugScratch,
        src: &'s [u8],
    ) -> hod_ir::IrResult<DiNode<'s>> {
        // `kit::di_node`, with the record's fields set by position.
        let Some(kind) = self.kind else {
            return kit::di_node(None, self.name, []);
        };
        let mut node = DiNode::llvm_default(kind);
        for &(key, value) in fields {
            let value = match value {
                Val::UInt(value) => DiFieldValue::UInt(value),
                Val::Int(value) => DiFieldValue::Int(value),
                Val::Bool(value) => DiFieldValue::Bool(value),
                Val::Src(start, end) => DiFieldValue::Str(&src[start as usize..end as usize]),
                Val::Buf(start, end) => {
                    DiFieldValue::Str(&scratch.strs[start as usize..end as usize])
                }
                Val::Node(id) => DiFieldValue::Node(id),
                Val::List(start, end) => {
                    DiFieldValue::List(&scratch.lists[start as usize..end as usize])
                }
                Val::Expr(id) => DiFieldValue::Expr(id),
            };
            match key {
                Key::At(index) => node.set_at(usize::from(index), value)?,
                Key::Name(field) => node.set(field.as_bytes(), value)?,
            }
        }
        Ok(node)
    }

    fn define<L: Layering>(
        &self,
        debug: &mut hod_ir::DebugInfo<L>,
        id: DiId,
        scratch: &DebugScratch,
        src: &[u8],
        distinct: bool,
    ) -> hod_ir::IrResult<()> {
        let node = self.node(&scratch.fields[self.first..], scratch, src)?;
        kit::define_di_node(debug, id, &node, distinct)
    }

    fn add<L: Layering>(
        &self,
        debug: &mut hod_ir::DebugInfo<L>,
        fields: &[(Key, Val)],
        scratch: &DebugScratch,
        src: &[u8],
    ) -> hod_ir::IrResult<DiId> {
        let node = self.node(fields, scratch, src)?;
        debug.add(&node)
    }
}

impl DebugMap {
    /// The handle of the node at `position` in the metadata index.
    pub(crate) fn handle(&self, position: usize) -> Handle {
        self.handles.get(position).copied().unwrap_or(Handle::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WP-290: the index's kind is the kind `DiKind::named` finds for
    /// every kind with a record, and none for the others; every required
    /// field has a position in its record.
    #[test]
    fn record_kinds_and_required_fields() {
        for &kind in DiKind::ALL {
            let name = kind.llvm_name().as_bytes();
            let md = MdKind::from_name(name);
            let expected = DiKind::named(name).filter(|_| name.starts_with(b"DI"));
            assert_eq!(record_kind(md), expected, "{kind:?}");
            for field in required(kind) {
                assert!(kind.field_index(field.as_bytes(), 0).is_some(), "{kind:?}.{field}");
            }
        }
        for name in
            [&b"DIModule"[..], b"DILocation", b"DIStringType", b"DIFuture", b"GenericDINode"]
        {
            assert_eq!(record_kind(MdKind::from_name(name)), None);
        }
        assert_eq!(required(DiKind::Label), ["scope", "name", "file", "line"]);
        assert!(required(DiKind::Subprogram).is_empty());
    }
}
