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

use crate::Span;
use crate::error::{Fail, LlErrorKind, PResult, fail};
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
    let mut token = scanner.next()?;
    loop {
        token = match token.kind {
            Tok::Eof => return Ok(scanner.index),
            Tok::Global | Tok::GlobalQuoted | Tok::GlobalId => {
                scanner.expect(Tok::Equal, "expected '=' after a global name")?;
                let start = scanner.lx.pos() as u32;
                let next = scanner.rest_of_entity()?;
                scanner.push_item(ItemKind::Assign, token, start, next.start);
                next
            }
            Tok::Local | Tok::LocalQuoted | Tok::LocalId => {
                scanner.expect(Tok::Equal, "expected '=' after a type name")?;
                scanner.expect_word(b"type", "expected 'type' after the name")?;
                let start = scanner.lx.pos() as u32;
                let next = scanner.rest_of_entity()?;
                scanner.index.types.push(TypeEnt { name: token, start, end: next.start });
                next
            }
            Tok::Comdat | Tok::ComdatQuoted => {
                scanner.expect(Tok::Equal, "expected '=' after a comdat name")?;
                scanner.expect_word(b"comdat", "expected 'comdat' here")?;
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

impl Scanner<'_> {
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

    fn expect_word(&mut self, word: &[u8], what: &'static str) -> PResult<Token> {
        let token = self.next()?;
        if token.kind != Tok::Word || self.lx.text(token) != word {
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
                self.expect_word(b"asm", "expected 'module asm'")?;
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
    use super::{parse_u32, parse_u64};

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
