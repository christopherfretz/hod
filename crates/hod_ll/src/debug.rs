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

/// The fields LLVM's parser requires (`LLParser`'s `REQUIRED`), by
/// kind; `DILocation`'s `scope` is checked where locations are read.
const REQUIRED: &[(DiKind, &str)] = &[
    (DiKind::File, "filename"),
    (DiKind::File, "directory"),
    (DiKind::Enumerator, "name"),
    (DiKind::Enumerator, "value"),
    (DiKind::DerivedType, "tag"),
    (DiKind::DerivedType, "baseType"),
    (DiKind::CompositeType, "tag"),
    (DiKind::SubroutineType, "types"),
    (DiKind::CompileUnit, "file"),
    (DiKind::LexicalBlock, "scope"),
    (DiKind::LexicalBlockFile, "scope"),
    (DiKind::LexicalBlockFile, "discriminator"),
    (DiKind::Namespace, "scope"),
    (DiKind::TemplateTypeParameter, "type"),
    (DiKind::TemplateValueParameter, "value"),
    (DiKind::GlobalVariableExpression, "var"),
    (DiKind::GlobalVariableExpression, "expr"),
    (DiKind::LocalVariable, "scope"),
    (DiKind::ImportedEntity, "tag"),
    (DiKind::ImportedEntity, "scope"),
    (DiKind::Label, "scope"),
    (DiKind::Label, "name"),
    (DiKind::Label, "file"),
    (DiKind::Label, "line"),
];

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

/// The scratch storage of the node being built and of those written in
/// place inside it.
#[derive(Debug, Default)]
pub(crate) struct DebugScratch {
    fields: Vec<(&'static str, Val)>,
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
        for at in 0..count {
            let node = self.md.nodes()[at];
            match self.dmap.handles[at] {
                // A tuple named as a node is defined by `define_tuples`.
                Handle::Node(_) if node.kind == MdKind::Tuple => {}
                Handle::Node(id) => {
                    let name = kind_name(self.src, node.span);
                    let made = self.node_body(name, node.span, &mut scratch)?;
                    let debug = &mut self.module.debug;
                    let result = made.define(debug, id, &scratch, self.src, node.distinct);
                    ir(node.span.start, result)?;
                    scratch.fields.clear();
                    scratch.lists.clear();
                    scratch.strs.clear();
                }
                Handle::Loc(id) => {
                    let (loc, atom, distinct) =
                        self.location_body(node.span, node.distinct, &mut scratch)?;
                    let debug = &mut self.module.debug;
                    let result = debug.define_loc_in_atom(id, loc, atom, distinct);
                    ir(node.span.start, result)?;
                }
                Handle::None | Handle::Expr(_) | Handle::AssignId => {}
            }
            self.define_tuples(&mut scratch)?;
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
        Ok(())
    }

    /// Defines the tuples fields named as single nodes, and those their
    /// elements named in turn.
    fn define_tuples(&mut self, scratch: &mut DebugScratch) -> PResult<()> {
        while let Some((id, span)) = scratch.tuples.pop() {
            let mark = scratch.lists.len();
            self.goto(span.start);
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
        let made = self.node_body(kind_name(src, span), span, scratch)?;
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
        name: &'a [u8],
        span: Span,
        scratch: &mut DebugScratch,
    ) -> PResult<Made<'a>> {
        let mapping = di_kind(name);
        let Some(kind) = DiKind::named(name).filter(|_| mapping == Some(DiMapping::Mapped)) else {
            if mapping.is_none() {
                let at = span.start - 2 - name.len() as u32;
                return fail(LlErrorKind::Syntax, at, "expected metadata type");
            }
            return Ok(Made { kind: None, name, first: scratch.fields.len() });
        };
        let first = scratch.fields.len();
        // The fields met, for LLVM's two rules: none twice, and the
        // required ones present.
        let mut seen: [&[u8]; 48] = [b""; 48];
        let mut count = 0;
        self.goto(span.start);
        while self.tok.start < span.end {
            let (field, at) = self.field_name()?;
            if seen[..count].contains(&field) {
                let field = String::from_utf8_lossy(field);
                let message = format!("field '{field}' cannot be specified more than once");
                return fail(LlErrorKind::Syntax, at, message);
            }
            if count < seen.len() {
                seen[count] = field;
                count += 1;
            }
            match di_field(name, field) {
                Some(DiMapping::Mapped) => {
                    let value = self.field_value(kind, field, scratch)?;
                    let key = static_field(kind, field);
                    scratch.fields.push((key, value));
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
        for &(required, field) in REQUIRED {
            if required == kind && !seen[..count].contains(&field.as_bytes()) {
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

    /// A field's value, read by what the field holds.
    fn field_value(
        &mut self,
        kind: DiKind,
        field: &[u8],
        scratch: &mut DebugScratch,
    ) -> PResult<Val> {
        let shape = kind.field_shape(field);
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
        let named = |text: &[u8]| named_code(shape, field, text);
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
            Tok::Word if text != b"null" && named(text).is_some() => {
                self.bump();
                Val::UInt(named(text).unwrap_or(0))
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

/// The `'static` name of a field LLVM calls `field`: one of the kind's
/// record fields, or one of the older spellings a subprogram takes.
fn static_field(kind: DiKind, field: &[u8]) -> &'static str {
    if field == b"tag" {
        return "tag";
    }
    if let Some(&known) = kind.fields().iter().find(|known| known.as_bytes() == field) {
        return known;
    }
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
        fields: &'s [(&'static str, Val)],
        scratch: &'s DebugScratch,
        src: &'s [u8],
    ) -> hod_ir::IrResult<DiNode<'s>> {
        let values = fields.iter().map(|&(field, value)| {
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
            (field, value)
        });
        kit::di_node(self.kind, self.name, values)
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
        fields: &[(&'static str, Val)],
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
