//! The index scan (`spec/ll.md` §4.1): one pass that finds every
//! top-level entity and where it ends, lexing only the tokens outside
//! brackets and stepping over bracketed groups and function bodies with
//! [`crate::skip`].
//!
//! An entity begins at a name followed by `=` (`@g =`, `%T =`, `$c =`,
//! `!name =`, `!7 =`, `^3 =`) or at one of the keywords `define`,
//! `declare`, `target triple`, `target datalayout`, `source_filename`,
//! `module`, `attributes`, `uselistorder`, `uselistorder_bb`, and ends
//! where the next one begins: line ends mean nothing, as in LLVM.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::Span;
use crate::error::{Fail, LlErrorKind, PResult, fail};
use crate::kw::Kw;
use crate::lexer::{Lexer, Tok, Token};
use crate::meta::MdKind;
use crate::skip::skip_group;

/// `%T = type …`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TypeEnt {
    pub name: Token,
    /// Offset after the `type` keyword.
    pub start: u32,
    /// Where the next entity starts.
    pub end: u32,
}

/// `$name = comdat kind`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ComdatEnt {
    pub name: Token,
    pub kind: Token,
}

/// `attributes #N = { … }`; the span is the inside of the braces.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GroupEnt {
    pub id: Token,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ItemKind {
    /// `@name = …`: a variable, an alias or an IFUNC.
    Assign,
    Declare,
    Define,
}

/// A global value, in file order.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Item {
    pub kind: ItemKind,
    /// The name token of an assignment or a definition; the keyword of a
    /// declaration.
    pub name: Token,
    /// Where its text starts: after the `=` or the keyword.
    pub start: u32,
    /// Where its text ends: the next entity, or the `{` of the body.
    pub end: u32,
    /// The inside of a definition's parameter parentheses.
    pub params: Span,
    /// The inside of a definition's body braces.
    pub body: Span,
}

/// `!N = [distinct] !{…}` or `!N = [distinct] !Kind(…)`; the span is the
/// inside of the braces or parentheses.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NodeEnt {
    pub id: u32,
    pub kind: MdKind,
    pub distinct: bool,
    pub span: Span,
    /// Offset of the `!` that starts the entity.
    pub at: u32,
}

/// `!name = !{…}`; the span is the inside of the braces.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NamedEnt {
    pub name: Token,
    pub span: Span,
}

/// Every top-level entity of a source.
#[derive(Debug, Default)]
pub(crate) struct Index {
    pub triple: Option<Token>,
    pub datalayout: Option<Token>,
    pub module_asm: Vec<Token>,
    pub types: Vec<TypeEnt>,
    pub comdats: Vec<ComdatEnt>,
    pub groups: Vec<GroupEnt>,
    pub items: Vec<Item>,
    pub nodes: Vec<NodeEnt>,
    pub named: Vec<NamedEnt>,
}

struct Scanner<'a> {
    lx: Lexer<'a>,
    index: Index,
}

/// Indexes `src`.
pub(crate) fn scan(src: &[u8]) -> PResult<Index> {
    let mut scanner = Scanner { lx: Lexer::new(src), index: Index::default() };
    let token = scanner.next()?;
    scanner.run::<false>(token, usize::MAX)?;
    Ok(scanner.index)
}

/// The fewest bytes of source a piece of a parallel scan has: a smaller
/// one costs about what starting and joining its thread does
/// (`spec/ll.md` §4.4, measured in `spec/landed/ll/WP-303.md`).
pub const SCAN_BYTES_PER_THREAD: usize = 1 << 20;

/// The most threads a scan runs on.
pub const SCAN_MAX_THREADS: usize = 8;

/// The bytes of source of a piece of a parallel scan: the threads take
/// pieces in turn, so that one whose text is slower to scan (many small
/// entities) does not leave the others waiting.
pub const SCAN_PIECE_BYTES: usize = 256 << 10;

/// How many threads [`scan_on`] scans `len` bytes of source with, when
/// `granted` may run: one per [`SCAN_BYTES_PER_THREAD`], at most
/// [`SCAN_MAX_THREADS`] and `granted`, at least one. A pure function of
/// the two.
#[must_use]
pub fn scan_threads(len: usize, granted: usize) -> usize {
    granted.min(SCAN_MAX_THREADS).min(len / SCAN_BYTES_PER_THREAD).max(1)
}

/// What one piece of a parallel scan found: the start of the first token
/// it met (the entity it began at), what it indexed, and how it ended:
/// at the token that begins the first entity at or after its stop, at
/// the end of the source (`None`), or at an error.
struct Piece {
    first: u32,
    index: Index,
    end: PResult<Option<Token>>,
}

/// Indexes `src` on up to `threads` threads (`spec/ll.md` §4.4): the
/// result, errors included, is that of [`scan`].
///
/// The source is cut about every [`SCAN_PIECE_BYTES`] at line starts
/// that look like the start of an entity; the threads (the calling one
/// among them) take the pieces in turn and scan each from its cut up to
/// the first entity at or after the next cut, and the pieces are joined
/// in source order. A piece is kept only if it began at the very token
/// the piece before it ended at: a scan from an entity's first token is
/// a function of the source and that token alone, so the piece is then
/// what the serial scan finds there. Any other piece is scanned again,
/// from that token, on the calling thread; so is everything after the
/// first piece that ended at an error, which is the error.
pub(crate) fn scan_on(src: &[u8], threads: usize) -> PResult<Index> {
    scan_split(src, scan_threads(src.len(), threads), SCAN_PIECE_BYTES)
}

/// [`scan_on`] on exactly `threads` threads (fewer when there are fewer
/// pieces), with pieces of about `piece_bytes`.
fn scan_split(src: &[u8], threads: usize, piece_bytes: usize) -> PResult<Index> {
    // `cuts[k]` is where piece `k + 1` begins; piece 0 begins at 0.
    let mut cuts = Vec::new();
    let mut target = piece_bytes;
    while threads > 1 && target < src.len() {
        let Some(cut) = cut_at(src, target) else { break };
        cuts.push(cut);
        target = cut + piece_bytes;
    }
    if cuts.is_empty() {
        return scan(src);
    }
    let start = |piece: usize| if piece == 0 { 0 } else { cuts[piece - 1] };
    let stop = |piece: usize| cuts.get(piece).copied().unwrap_or(usize::MAX);
    let next = AtomicUsize::new(0);
    let take = || {
        let mut done = Vec::new();
        loop {
            let piece = next.fetch_add(1, Ordering::Relaxed);
            if piece > cuts.len() {
                return done;
            }
            done.push((piece, scan_piece(src, start(piece), stop(piece))));
        }
    };
    let mut pieces: Vec<Option<Piece>> = (0..=cuts.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (1..threads.min(cuts.len() + 1)).map(|_| scope.spawn(take)).collect();
        let mut done = take();
        for worker in workers {
            done.extend(worker.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)));
        }
        for (piece, found) in done {
            pieces[piece] = Some(found);
        }
    });
    let mut pieces = pieces.into_iter().flatten();
    let first = pieces.next().expect("piece 0 is scanned");
    let mut index = first.index;
    let mut end = first.end;
    for (at, piece) in pieces.enumerate() {
        let token = match end {
            Err(error) => return Err(error),
            Ok(None) => break,
            Ok(Some(token)) => token,
        };
        if piece.first == token.start {
            index.append(piece.index);
            end = piece.end;
        } else {
            let mut scanner = Scanner { lx: Lexer::new(src), index };
            scanner.lx.set_pos(token.end as usize);
            end = scanner.run::<true>(token, stop(at + 1));
            index = scanner.index;
        }
    }
    end.map(|_| index)
}

/// Scans `src` from `from` up to the first entity at or after `stop`.
fn scan_piece(src: &[u8], from: usize, stop: usize) -> Piece {
    let mut scanner = Scanner { lx: Lexer::new(src), index: Index::default() };
    scanner.lx.set_pos(from);
    let token = scanner.lx.next_token();
    let first = token.start;
    let end = if token.kind == Tok::Error {
        fail(LlErrorKind::Lex, token.start, scanner.lx.error())
    } else {
        scanner.run::<true>(token, stop)
    };
    Piece { first, index: scanner.index, end }
}

/// The first line start at or after `target` whose first byte is `!` or
/// `@`, or that begins with `define `: where a piece of a parallel scan
/// begins. Any offset would be correct (a piece that began elsewhere
/// than an entity is scanned again); these are where one most often
/// begins in LLVM's text.
fn cut_at(src: &[u8], target: usize) -> Option<usize> {
    let mut at = target;
    loop {
        let line = at + src.get(at..)?.iter().position(|&byte| byte == b'\n')? + 1;
        match src.get(line) {
            Some(b'!' | b'@') => return Some(line),
            Some(b'd') if src[line..].starts_with(b"define ") => return Some(line),
            Some(_) => at = line,
            None => return None,
        }
    }
}

impl Index {
    /// Adds what `later`, the index of the text after this one's, holds.
    fn append(&mut self, mut later: Index) {
        if later.triple.is_some() {
            self.triple = later.triple;
        }
        if later.datalayout.is_some() {
            self.datalayout = later.datalayout;
        }
        self.module_asm.append(&mut later.module_asm);
        self.types.append(&mut later.types);
        self.comdats.append(&mut later.comdats);
        self.groups.append(&mut later.groups);
        self.items.append(&mut later.items);
        self.nodes.append(&mut later.nodes);
        self.named.append(&mut later.named);
    }
}

impl Scanner<'_> {
    /// The scan from `token`, the first of an entity, to the end of the
    /// source; `BOUNDED`, only up to the first entity that begins at or
    /// after `stop`, whose first token is the answer.
    #[allow(clippy::inline_always)] // The serial scan is this loop, as before WP-303.
    #[inline(always)]
    fn run<const BOUNDED: bool>(
        &mut self,
        mut token: Token,
        stop: usize,
    ) -> PResult<Option<Token>> {
        let scanner = self;
        loop {
            if BOUNDED && token.start as usize >= stop && token.kind != Tok::Eof {
                return Ok(Some(token));
            }
            token = match token.kind {
                Tok::Eof => return Ok(None),
                Tok::Global | Tok::GlobalQuoted | Tok::GlobalId => {
                    scanner.expect(Tok::Equal, "expected '=' after a global name")?;
                    let start = scanner.lx.pos() as u32;
                    let next = scanner.rest_of_entity()?;
                    scanner.push_item(ItemKind::Assign, token, start, next.start);
                    next
                }
                Tok::Local | Tok::LocalQuoted | Tok::LocalId => {
                    scanner.expect(Tok::Equal, "expected '=' after a type name")?;
                    scanner.expect_word(Kw::Type, "expected 'type' after the name")?;
                    let start = scanner.lx.pos() as u32;
                    let next = scanner.rest_of_entity()?;
                    scanner.index.types.push(TypeEnt { name: token, start, end: next.start });
                    next
                }
                Tok::Comdat | Tok::ComdatQuoted => {
                    scanner.expect(Tok::Equal, "expected '=' after a comdat name")?;
                    scanner.expect_word(Kw::Comdat, "expected 'comdat' here")?;
                    let kind = scanner.expect(Tok::Word, "expected a comdat selection kind")?;
                    scanner.index.comdats.push(ComdatEnt { name: token, kind });
                    scanner.next()?
                }
                Tok::Meta => {
                    scanner.expect(Tok::Equal, "expected '=' after a metadata name")?;
                    scanner.expect(Tok::Exclaim, "expected '!' here")?;
                    let open = scanner.expect(Tok::LBrace, "expected '{' here")?;
                    let span = scanner.group(open)?;
                    scanner.index.named.push(NamedEnt { name: token, span });
                    scanner.next()?
                }
                Tok::Exclaim => scanner.node(token)?,
                Tok::SummaryId => {
                    scanner.expect(Tok::Equal, "expected '=' after a summary id")?;
                    scanner.rest_of_entity()?
                }
                Tok::Word => scanner.keyword(token)?,
                _ => return fail(LlErrorKind::Syntax, token.start, "expected a top-level entity"),
            };
        }
    }

    fn next(&mut self) -> PResult<Token> {
        let token = self.lx.next_token();
        if token.kind == Tok::Error {
            return fail(LlErrorKind::Lex, token.start, self.lx.error());
        }
        Ok(token)
    }

    fn expect(&mut self, kind: Tok, what: &'static str) -> PResult<Token> {
        let token = self.next()?;
        if token.kind != kind {
            return fail(LlErrorKind::Syntax, token.start, what);
        }
        Ok(token)
    }

    fn expect_word(&mut self, word: Kw, what: &'static str) -> PResult<Token> {
        let token = self.next()?;
        if token.kind != Tok::Word || token.kw != word {
            return fail(LlErrorKind::Syntax, token.start, what);
        }
        Ok(token)
    }

    fn push_item(&mut self, kind: ItemKind, name: Token, start: u32, end: u32) {
        let none = Span { start: end, end };
        self.index.items.push(Item { kind, name, start, end, params: none, body: none });
    }

    /// Steps over the group that `open` opens; the span of its inside.
    fn group(&mut self, open: Token) -> PResult<Span> {
        let src = self.lx.source();
        let from = open.end as usize;
        let end = match open.kind {
            Tok::LParen => skip_group::<b'(', b')'>(src, from),
            Tok::LBracket => skip_group::<b'[', b']'>(src, from),
            Tok::Less => skip_group::<b'<', b'>'>(src, from),
            _ => skip_group::<b'{', b'}'>(src, from),
        };
        match end {
            Ok(end) => {
                self.lx.set_pos(end);
                Ok(Span { start: open.end, end: end as u32 - 1 })
            }
            Err(at) if at >= src.len() => Err(Box::new(Fail {
                kind: LlErrorKind::Syntax,
                offset: open.start,
                message: "this bracket is never closed".into(),
            })),
            Err(at) => fail(LlErrorKind::Lex, at as u32, "unterminated string or comment"),
        }
    }

    /// Whether the token after the current position is `kind`; the
    /// position does not move.
    fn followed_by(&mut self, kind: Tok) -> bool {
        let pos = self.lx.pos();
        let found = self.lx.next_token().kind == kind;
        self.lx.set_pos(pos);
        found
    }

    /// Whether `token`, met outside brackets, begins an entity.
    fn begins_entity(&mut self, token: Token) -> bool {
        match token.kind {
            Tok::Global
            | Tok::GlobalQuoted
            | Tok::GlobalId
            | Tok::Local
            | Tok::LocalQuoted
            | Tok::LocalId
            | Tok::Comdat
            | Tok::ComdatQuoted
            | Tok::Meta
            | Tok::SummaryId => self.followed_by(Tok::Equal),
            Tok::Exclaim => {
                let pos = self.lx.pos();
                let found = self.lx.next_token().kind == Tok::Int
                    && self.lx.next_token().kind == Tok::Equal;
                self.lx.set_pos(pos);
                found
            }
            Tok::Word => match self.lx.text(token) {
                b"define" | b"declare" | b"attributes" | b"source_filename" | b"module"
                | b"uselistorder" | b"uselistorder_bb" => true,
                b"target" => {
                    let pos = self.lx.pos();
                    let next = self.lx.next_token();
                    self.lx.set_pos(pos);
                    next.kind == Tok::Word
                        && matches!(self.lx.text(next), b"triple" | b"datalayout")
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Steps to the end of the current entity; the token that begins the
    /// next one, or the end of the input.
    fn rest_of_entity(&mut self) -> PResult<Token> {
        loop {
            let token = self.next()?;
            match token.kind {
                Tok::Eof => return Ok(token),
                Tok::LParen | Tok::LBracket | Tok::LBrace => {
                    self.group(token)?;
                }
                _ if self.begins_entity(token) => return Ok(token),
                _ => {}
            }
        }
    }

    fn keyword(&mut self, token: Token) -> PResult<Token> {
        match self.lx.text(token) {
            b"define" => self.define(),
            b"declare" => {
                let next = self.rest_of_entity()?;
                self.push_item(ItemKind::Declare, token, token.end, next.start);
                Ok(next)
            }
            b"target" => {
                let what = self.expect(Tok::Word, "expected 'triple' or 'datalayout'")?;
                if !matches!(self.lx.text(what), b"triple" | b"datalayout") {
                    return fail(
                        LlErrorKind::Syntax,
                        what.start,
                        "expected 'triple' or 'datalayout'",
                    );
                }
                self.expect(Tok::Equal, "expected '=' after the target property")?;
                let text = self.expect(Tok::Str, "expected a string")?;
                match self.lx.text(what) {
                    b"triple" => self.index.triple = Some(text),
                    _ => self.index.datalayout = Some(text),
                }
                self.next()
            }
            b"source_filename" => {
                self.expect(Tok::Equal, "expected '=' after source_filename")?;
                self.expect(Tok::Str, "expected a string")?;
                self.next()
            }
            b"module" => {
                self.expect_word(Kw::Asm, "expected 'module asm'")?;
                let text = self.expect(Tok::Str, "expected a string")?;
                self.index.module_asm.push(text);
                self.next()
            }
            b"attributes" => {
                let id = self.expect(Tok::AttrGroup, "expected an attribute group id")?;
                self.expect(Tok::Equal, "expected '=' here")?;
                let open = self.expect(Tok::LBrace, "expected '{' here")?;
                let span = self.group(open)?;
                self.index.groups.push(GroupEnt { id, span });
                self.next()
            }
            b"uselistorder" | b"uselistorder_bb" => self.rest_of_entity(),
            _ => fail(LlErrorKind::Syntax, token.start, "expected a top-level entity"),
        }
    }

    /// `!N = [distinct] !{…}` or `!N = [distinct] !Kind(…)`.
    fn node(&mut self, bang: Token) -> PResult<Token> {
        let id_token = self.expect(Tok::Int, "expected a metadata node number")?;
        let Some(id) = parse_u32(self.lx.text(id_token)) else {
            return fail(LlErrorKind::Syntax, id_token.start, "expected a metadata node number");
        };
        self.expect(Tok::Equal, "expected '=' here")?;
        let mut token = self.next()?;
        let distinct = token.kind == Tok::Word && self.lx.text(token) == b"distinct";
        if distinct {
            token = self.next()?;
        }
        let (kind, open) = match token.kind {
            Tok::Exclaim => (MdKind::Tuple, self.expect(Tok::LBrace, "expected '{' here")?),
            Tok::Meta => (
                MdKind::from_name(&self.lx.text(token)[1..]),
                self.expect(Tok::LParen, "expected '(' here")?,
            ),
            _ => return fail(LlErrorKind::Syntax, token.start, "expected a metadata node"),
        };
        let span = self.group(open)?;
        self.index.nodes.push(NodeEnt { id, kind, distinct, span, at: bang.start });
        self.next()
    }

    /// `define`: finds the name, the parameter list and the body. The `{`
    /// of the body is the first one outside brackets after the parameter
    /// list that is not part of the typed constant of a `prefix`,
    /// `prologue` or `personality`.
    fn define(&mut self) -> PResult<Token> {
        let start = self.lx.pos() as u32;
        let name = loop {
            let token = self.next()?;
            match token.kind {
                Tok::Global | Tok::GlobalQuoted | Tok::GlobalId => break token,
                Tok::LParen | Tok::LBracket | Tok::LBrace => {
                    self.group(token)?;
                }
                Tok::Eof => {
                    return fail(LlErrorKind::Syntax, token.start, "expected a function name");
                }
                _ => {}
            }
        };
        let open = self.expect(Tok::LParen, "expected '(' in a function's parameter list")?;
        let params = self.group(open)?;
        let brace = loop {
            let token = self.next()?;
            match token.kind {
                Tok::LBrace => break token,
                Tok::LParen | Tok::LBracket => {
                    self.group(token)?;
                }
                Tok::Word
                    if matches!(self.lx.text(token), b"prefix" | b"prologue" | b"personality") =>
                {
                    self.skim_type()?;
                    self.skim_value()?;
                }
                Tok::Eof => {
                    return fail(LlErrorKind::Syntax, token.start, "expected '{' in function body");
                }
                _ => {}
            }
        };
        let body = self.group(brace)?;
        self.index.items.push(Item {
            kind: ItemKind::Define,
            name,
            start,
            end: brace.start,
            params,
            body,
        });
        self.next()
    }

    /// Steps over one type.
    fn skim_type(&mut self) -> PResult<()> {
        let token = self.next()?;
        match token.kind {
            Tok::LBrace | Tok::LBracket | Tok::Less => {
                self.group(token)?;
            }
            Tok::Word | Tok::IntType | Tok::Local | Tok::LocalQuoted | Tok::LocalId => {}
            _ => return fail(LlErrorKind::Syntax, token.start, "expected a type"),
        }
        loop {
            let pos = self.lx.pos();
            let token = self.lx.next_token();
            match token.kind {
                Tok::Star => {}
                Tok::LParen => {
                    self.group(token)?;
                }
                Tok::Word if self.lx.text(token) == b"addrspace" => {}
                _ => {
                    self.lx.set_pos(pos);
                    return Ok(());
                }
            }
        }
    }

    /// Steps over one constant.
    fn skim_value(&mut self) -> PResult<()> {
        let token = self.next()?;
        match token.kind {
            Tok::LBrace | Tok::LBracket | Tok::Less => {
                self.group(token)?;
                Ok(())
            }
            Tok::Global
            | Tok::GlobalQuoted
            | Tok::GlobalId
            | Tok::Int
            | Tok::HexInt
            | Tok::Float
            | Tok::HexDouble
            | Tok::HexFp80
            | Tok::HexFp128
            | Tok::HexPpcFp128
            | Tok::HexHalf
            | Tok::HexBFloat => Ok(()),
            Tok::Word => match self.lx.text(token) {
                b"c" => self.expect(Tok::Str, "expected a string").map(drop),
                b"dso_local_equivalent" | b"no_cfi" => self.next().map(drop),
                word if is_const_operator(word) => loop {
                    let token = self.next()?;
                    match token.kind {
                        Tok::Word if self.lx.text(token) == b"inrange" => {
                            let open = self.expect(Tok::LParen, "expected '(' here")?;
                            self.group(open)?;
                        }
                        Tok::Word => {}
                        Tok::LParen => {
                            self.group(token)?;
                            return Ok(());
                        }
                        _ => {
                            return fail(LlErrorKind::Syntax, token.start, "expected '(' here");
                        }
                    }
                },
                _ => Ok(()),
            },
            _ => fail(LlErrorKind::Syntax, token.start, "expected a value"),
        }
    }
}

/// Whether `word` is an operator of a constant expression, which is
/// followed by flags and one group in parentheses.
pub(crate) fn is_const_operator(word: &[u8]) -> bool {
    matches!(
        word,
        b"getelementptr"
            | b"trunc"
            | b"ptrtoint"
            | b"ptrtoaddr"
            | b"inttoptr"
            | b"bitcast"
            | b"addrspacecast"
            | b"add"
            | b"sub"
            | b"mul"
            | b"xor"
            | b"shl"
            | b"extractelement"
            | b"insertelement"
            | b"shufflevector"
            | b"blockaddress"
            | b"splat"
            | b"ptrauth"
    )
}

/// A decimal number that fits 32 bits.
pub(crate) fn parse_u32(digits: &[u8]) -> Option<u32> {
    u32::try_from(parse_u64(digits)?).ok()
}

/// A decimal number that fits 64 bits.
pub(crate) fn parse_u64(digits: &[u8]) -> Option<u64> {
    if digits.is_empty() {
        return None;
    }
    // Nineteen digits always fit: no overflow test per digit.
    if digits.len() <= 19 {
        let mut value = 0u64;
        for &d in digits {
            let digit = d.wrapping_sub(b'0');
            if digit > 9 {
                return None;
            }
            value = value * 10 + u64::from(digit);
        }
        return Some(value);
    }
    let mut value = 0u64;
    for &d in digits {
        if !d.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u64::from(d - b'0'))?;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::{parse_u32, parse_u64, scan, scan_split};

    /// The serial scan's answer and that of a scan in pieces of
    /// `piece_bytes` on `threads` threads, as text.
    fn both(src: &[u8], threads: usize, piece_bytes: usize) -> (String, String) {
        (format!("{:?}", scan(src)), format!("{:?}", scan_split(src, threads, piece_bytes)))
    }

    /// The texts: every seed of `tests/ir` and every `.ll` of this
    /// crate's test data, debug information among them.
    fn texts() -> Vec<(String, Vec<u8>)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut texts = Vec::new();
        for dir in
            [root.join("../../tests/ir"), root.join("tests/data"), root.join("tests/data/debug")]
        {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
            paths.sort();
            for path in paths {
                if path.extension().is_some_and(|ext| ext == "ll") {
                    texts.push((path.display().to_string(), std::fs::read(&path).unwrap()));
                }
            }
        }
        assert!(texts.len() > 50, "the seeds are found");
        texts
    }

    /// §4.4: a scan in pieces is the serial scan, for every text, piece
    /// size and thread count.
    #[test]
    fn pieces_are_the_serial_scan() {
        for (name, src) in texts() {
            for (threads, piece) in [(2, 64), (3, 200), (8, 1000), (4, 4096), (24, 97)] {
                let (serial, pieces) = both(&src, threads, piece);
                assert_eq!(serial, pieces, "{name}, {threads} threads, pieces of {piece}");
            }
        }
    }

    /// §4.4: with an error, the first in source order is the answer, at
    /// the same offset and with the same message: a stray byte or an
    /// unclosed bracket put at many places of a text with debug
    /// information, one or two at a time.
    #[test]
    fn pieces_report_the_first_error() {
        let texts = texts();
        let (name, src) = texts
            .iter()
            .filter(|(_, src)| src.windows(13).any(|w| w == b"DICompileUnit"))
            .max_by_key(|(_, src)| src.len())
            .expect("a text with debug information");
        let step = src.len() / 61 + 1;
        for at in (0..src.len()).step_by(step) {
            for (bad, second) in
                [(b'`', None), (b'{', None), (b'"', None), (b'`', Some(at + step / 2))]
            {
                let mut text = src.clone();
                text[at] = bad;
                if let Some(second) = second.filter(|&second| second < text.len()) {
                    text[second] = b'`';
                }
                for (threads, piece) in [(2, 128), (8, 700)] {
                    let (serial, pieces) = both(&text, threads, piece);
                    assert_eq!(serial, pieces, "{name}: {bad:?} at {at}, {threads} threads");
                }
            }
            let (serial, pieces) = both(&src[..at], 4, 300);
            assert_eq!(serial, pieces, "{name} cut at {at}");
        }
    }

    /// §4.4: pieces that begin inside a comment, a string or an entity
    /// (a line start that looks like an entity and is not one) are
    /// scanned again.
    #[test]
    fn pieces_that_begin_inside_are_scanned_again() {
        use std::fmt::Write as _;
        let mut text = String::from("target triple = \"aarch64-unknown-linux-gnu\"\n");
        for n in 0..200 {
            let _ = write!(
                text,
                "!{n} = !{{i32 {n}}}\n/* a comment\n!{n} = !{{}}\n@x = */\n\
                 @g{n} = global [2 x i8] c\"!\\0A\"\n\
                 define void @f{n}(\n!7 = i32 1) {{\n  ret void\n}}\n"
            );
        }
        for (threads, piece) in [(2, 10), (5, 33), (8, 150)] {
            let (serial, pieces) = both(text.as_bytes(), threads, piece);
            assert!(serial.starts_with("Ok("), "{serial}");
            assert_eq!(serial, pieces, "{threads} threads, pieces of {piece}");
        }
    }

    /// §2.4: decimal numbers, at every length around the one up to which
    /// no digit is tested for overflow, against `str::parse`.
    #[test]
    fn decimal_numbers() {
        for text in [
            "0",
            "7",
            "4294967295",
            "4294967296",
            "999999999999999999",
            "9999999999999999999",
            "18446744073709551615",
            "18446744073709551616",
            "99999999999999999999",
            "00000000000000000000001",
            "",
            "1a",
            "-1",
            "12345678901234567x",
        ] {
            let expected = if text.bytes().all(|b| b.is_ascii_digit()) {
                text.parse::<u64>().ok()
            } else {
                None
            };
            assert_eq!(parse_u64(text.as_bytes()), expected, "{text:?}");
            assert_eq!(
                parse_u32(text.as_bytes()),
                expected.and_then(|v| u32::try_from(v).ok()),
                "{text:?}"
            );
        }
    }
}
