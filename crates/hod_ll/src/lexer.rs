//! The lexer (`spec/ll.md` §2): LLVM's token rules over a byte slice.
//!
//! A token is a kind and a byte span; nothing is copied or allocated, and
//! names and strings are unescaped only by whoever needs their bytes
//! ([`unescape`]). The rules are `LLLexer`'s, including the odd ones: a
//! run of label characters followed by `:` is a label whatever it looks
//! like, `i` followed by digits is an integer type even inside a longer
//! word, a word ends at the first character that is not alphanumeric or
//! `_`, and a string ends at the first `"`.

use crate::kw::{Kw, by_key, keyword_at};
use crate::skip::{comment_end, find_byte};

/// The kind of a [`Token`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum Tok {
    /// The end of the input.
    Eof,
    /// No token; [`Lexer::error`] says why.
    Error,
    /// `@name`.
    Global,
    /// `@"name"`, with `\xx` escapes.
    GlobalQuoted,
    /// `@42`.
    GlobalId,
    /// `%name`.
    Local,
    /// `%"name"`.
    LocalQuoted,
    /// `%42`.
    LocalId,
    /// `$name`.
    Comdat,
    /// `$"name"`.
    ComdatQuoted,
    /// `!name`: a metadata name (`!dbg`, `!llvm.module.flags`,
    /// `!DILocation`), with `\xx` escapes.
    Meta,
    /// `!` alone, as before `{`, a string or a node number.
    Exclaim,
    /// `#42`: an attribute group.
    AttrGroup,
    /// `#name`: the head of a debug record (`#dbg_value`).
    HashWord,
    /// `#` alone.
    Hash,
    /// `^42`: a module summary entry.
    SummaryId,
    /// A keyword or any other bare word: `define`, `x`, `DW_TAG_member`.
    Word,
    /// `iN`.
    IntType,
    /// `name:`; the span includes the colon.
    Label,
    /// `"name":`.
    LabelQuoted,
    /// `42:`.
    LabelId,
    /// A decimal integer, with an optional `-`.
    Int,
    /// `u0x…` or `s0x…`: a hexadecimal integer.
    HexInt,
    /// A decimal floating-point number: `1.0`, `-2.5e+10`, `+0.5`.
    Float,
    /// `0x…`: the bits of a `double`.
    HexDouble,
    /// `0xK…`: the bits of an `x86_fp80`.
    HexFp80,
    /// `0xL…`: the bits of an `fp128`.
    HexFp128,
    /// `0xM…`: the bits of a `ppc_fp128`.
    HexPpcFp128,
    /// `0xH…`: the bits of a `half`.
    HexHalf,
    /// `0xR…`: the bits of a `bfloat`.
    HexBFloat,
    /// `"…"`; the span includes the quotes.
    Str,
    /// `=`.
    Equal,
    /// `,`.
    Comma,
    /// `*`.
    Star,
    /// `[`.
    LBracket,
    /// `]`.
    RBracket,
    /// `{`.
    LBrace,
    /// `}`.
    RBrace,
    /// `<`.
    Less,
    /// `>`.
    Greater,
    /// `(`.
    LParen,
    /// `)`.
    RParen,
    /// `|`.
    Bar,
    /// `:`.
    Colon,
    /// `...`.
    DotDotDot,
}

/// One token: its kind, its keyword code and the bytes `start..end` of
/// the source.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Token {
    /// What it is.
    pub kind: Tok,
    /// For a [`Tok::Word`] or a [`Tok::IntType`], the keyword it is, if
    /// it is one (`spec/ll.md` §2.2); [`Kw::Other`] for every other token.
    pub kw: Kw,
    /// Offset of its first byte.
    pub start: u32,
    /// Offset one past its last byte.
    pub end: u32,
}

/// The largest input the lexer takes: offsets are 32 bits.
pub const MAX_SOURCE_BYTES: usize = u32::MAX as usize;

const LABEL: u8 = 1;
const DIGIT: u8 = 2;
const HEX: u8 = 4;
const WORD: u8 = 8;
const ALPHA: u8 = 16;
const BLANK: u8 = 32;
const META: u8 = 64;
const SKIP: u8 = 128;

/// Character classes: `LABEL` is `[-a-zA-Z$._0-9]`, `WORD` is
/// `[a-zA-Z0-9_]`, `BLANK` is white space and the zero byte, `META` is
/// what a metadata name is made of (`LABEL` and `\\`), `SKIP` is what
/// may start white space or a comment (`BLANK`, `;` and `/`).
const CLASS: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut c = 0usize;
    while c < 256 {
        let b = c as u8;
        let mut class = 0;
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'$' || b == b'.' || b == b'_' {
            class |= LABEL;
        }
        if b.is_ascii_digit() {
            class |= DIGIT;
        }
        if b.is_ascii_hexdigit() {
            class |= HEX;
        }
        if b.is_ascii_alphanumeric() || b == b'_' {
            class |= WORD;
        }
        if b.is_ascii_alphabetic() {
            class |= ALPHA;
        }
        if matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0) {
            class |= BLANK;
        }
        if class & LABEL != 0 || b == b'\\' {
            class |= META;
        }
        if class & BLANK != 0 || b == b';' || b == b'/' {
            class |= SKIP;
        }
        table[c] = class;
        c += 1;
    }
    table
};

#[inline]
fn is(byte: u8, class: u8) -> bool {
    CLASS[byte as usize] & class != 0
}

/// The lexer: a position in a source.
#[derive(Clone, Debug)]
pub struct Lexer<'a> {
    /// The bytes tokens are read from: the source, or nothing for a
    /// source larger than [`MAX_SOURCE_BYTES`], so that the size is
    /// looked at once and not per token.
    src: &'a [u8],
    /// The source as given.
    full: &'a [u8],
    pos: usize,
    /// Why the last [`Tok::Error`] was one.
    error: &'static str,
    /// A run of label characters already looked at: it starts at or
    /// before `run_start` and its first non-label byte is at `run_end`.
    /// Without it, `-1-1-1…` costs a scan of the rest of the run per
    /// token.
    run_start: usize,
    run_end: usize,
    /// Tokens already lexed by another lexer over the same source, each
    /// with the offset it was looked for at, in order of that offset
    /// (`spec/ll.md` §2.8): a token is a function of the source and that
    /// offset alone, so one looked for at the same offset is the same.
    replay: Vec<Replayed>,
    /// The next entry of `replay` that may still be wanted.
    replay_at: usize,
    /// The offset that entry was looked for at; `usize::MAX` when there
    /// is none, so the test per token is one compare.
    replay_from: usize,
}

/// A token and the offset its lexer looked for it at.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Replayed {
    from: u32,
    token: Token,
}

/// A record of tokens to replay ([`Lexer::replay`]): what [`Lexer::record`]
/// gives and what it is filled by.
#[derive(Debug, Default)]
pub(crate) struct Recording(Vec<Replayed>);

impl Recording {
    /// The most tokens kept, 16 bytes each: past them a scan goes on
    /// unrecorded and those tokens are lexed again.
    pub(crate) const MAX: usize = 1 << 16;

    /// Keeps `token`, looked for at `from`, while there is room.
    #[inline]
    pub(crate) fn push(&mut self, from: usize, token: Token) {
        if self.0.len() < Self::MAX {
            self.0.push(Replayed { from: from as u32, token });
        }
    }
}

impl<'a> Lexer<'a> {
    /// A lexer at the start of `src`. A source longer than
    /// [`MAX_SOURCE_BYTES`] lexes as a single [`Tok::Error`].
    pub fn new(src: &'a [u8]) -> Lexer<'a> {
        let full = src;
        let src = if src.len() > MAX_SOURCE_BYTES { &src[..0] } else { src };
        Lexer {
            src,
            full,
            pos: 0,
            error: "",
            run_start: usize::MAX,
            run_end: 0,
            replay: Vec::new(),
            replay_at: 0,
            replay_from: usize::MAX,
        }
    }

    /// Takes the tokens left to replay, emptied, to be filled again: the
    /// storage goes from one scan to the next.
    pub(crate) fn record(&mut self) -> Recording {
        let mut tokens = std::mem::take(&mut self.replay);
        tokens.clear();
        self.replay_at = 0;
        self.replay_from = usize::MAX;
        Recording(tokens)
    }

    /// Keeps the storage of `recording` for the next [`Lexer::record`],
    /// replaying none of it.
    pub(crate) fn reuse(&mut self, recording: Recording) {
        let mut tokens = recording.0;
        tokens.clear();
        self.replay = tokens;
        self.replay_at = 0;
        self.replay_from = usize::MAX;
    }

    /// From now on a token looked for at an offset `recording` has one
    /// for is that one, not lexed again. `recording` must come from a
    /// lexer over the same source, every token it holds be neither
    /// [`Tok::Eof`] nor [`Tok::Error`], and their offsets increase.
    pub(crate) fn replay(&mut self, recording: Recording) {
        debug_assert!(recording.0.windows(2).all(|pair| pair[0].from < pair[1].from));
        self.replay = recording.0;
        self.replay_at = 0;
        self.replay_from = self.replay.first().map_or(usize::MAX, |entry| entry.from as usize);
    }

    /// The recorded token looked for at `pos`, if there is one; skips the
    /// entries behind `pos`, which a lexer that goes forward never wants.
    #[inline]
    fn replayed(&mut self) -> Option<Token> {
        while let Some(entry) = self.replay.get(self.replay_at) {
            let from = entry.from as usize;
            if from > self.pos {
                self.replay_from = from;
                return None;
            }
            self.replay_at += 1;
            if from == self.pos {
                let token = entry.token;
                self.replay_from =
                    self.replay.get(self.replay_at).map_or(usize::MAX, |next| next.from as usize);
                self.pos = token.end as usize;
                return Some(token);
            }
        }
        self.replay_from = usize::MAX;
        None
    }

    /// The source.
    pub fn source(&self) -> &'a [u8] {
        self.full
    }

    /// The offset the next token is looked for at.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Moves to `pos`, which must be a token boundary.
    pub fn set_pos(&mut self, pos: usize) {
        self.pos = pos.min(self.src.len());
    }

    /// Why the last [`Tok::Error`] token was not a token.
    pub fn error(&self) -> &'static str {
        self.error
    }

    /// The bytes of `token`.
    pub fn text(&self, token: Token) -> &'a [u8] {
        &self.src[token.start as usize..token.end as usize]
    }

    /// The byte at `at`, or zero past the end (as LLVM's buffers end).
    #[inline]
    fn at(&self, at: usize) -> u8 {
        self.src.get(at).copied().unwrap_or(0)
    }

    /// The first byte at or after `from` that is not a label character.
    fn label_run_end(&mut self, from: usize) -> usize {
        if from >= self.run_start && from <= self.run_end {
            return self.run_end;
        }
        let end = self.skip_while(from, LABEL);
        self.run_start = from;
        self.run_end = end;
        end
    }

    /// Whether the label characters from `from` on end in a colon; the
    /// offset after it.
    fn label_tail(&mut self, from: usize) -> Option<usize> {
        // Most words and numbers end at a byte that neither goes on a
        // label nor ends one: no run to look at (and none to remember).
        let next = self.at(from);
        if !is(next, LABEL) && next != b':' {
            return None;
        }
        let end = self.label_run_end(from);
        (self.at(end) == b':').then_some(end + 1)
    }

    /// The first offset at or after `at` whose byte is not of `class`
    /// (the end of the source has none).
    ///
    /// Eight bytes at a time while eight are left, so that a byte costs
    /// its class test and not also an end test, then byte by byte.
    #[inline]
    fn skip_while(&self, mut at: usize, class: u8) -> usize {
        while let Some(chunk) = self.src.get(at..).and_then(<[u8]>::first_chunk::<8>) {
            if let Some(n) = chunk.iter().position(|&b| !is(b, class)) {
                return at + n;
            }
            at += 8;
        }
        let rest = self.src.get(at..).unwrap_or_default();
        at + rest.iter().position(|&b| !is(b, class)).unwrap_or(rest.len())
    }

    fn token(&mut self, kind: Tok, start: usize, end: usize) -> Token {
        self.pos = end;
        Token { kind, kw: Kw::Other, start: start as u32, end: end as u32 }
    }

    /// A word or an integer type, with its keyword code.
    #[inline]
    fn keyword_token(&mut self, kind: Tok, start: usize, end: usize) -> Token {
        self.pos = end;
        let kw = keyword_at(self.src, start, end);
        Token { kind, kw, start: start as u32, end: end as u32 }
    }

    fn fail(&mut self, why: &'static str, start: usize, end: usize) -> Token {
        self.error = why;
        self.token(Tok::Error, start, end.min(self.src.len()).max(start))
    }

    /// Skips white space and comments; `Err` is an error token.
    #[allow(clippy::inline_always)] // Measured: a call per token otherwise (WP-168).
    #[inline(always)]
    fn skip_blank(&mut self) -> Result<usize, Token> {
        let src = self.src;
        let mut pos = self.pos;
        // Tokens most often follow one another directly or after one
        // space.
        for _ in 0..2 {
            match src.get(pos) {
                Some(&b) if !is(b, SKIP) => return Ok(pos),
                Some(b' ') => pos += 1,
                _ => break,
            }
        }
        loop {
            pos = self.skip_while(pos, BLANK);
            match src.get(pos) {
                Some(b';') => pos = find_byte(src, pos, b'\n').unwrap_or(src.len()),
                Some(b'/') if src.get(pos + 1) == Some(&b'*') => {
                    let Some(end) = comment_end(src, pos + 2) else {
                        return Err(self.fail("unterminated /* comment", pos, pos + 2));
                    };
                    pos = end;
                }
                _ => return Ok(pos),
            }
        }
    }

    /// The next token. After [`Tok::Eof`] every call returns it again; a
    /// [`Tok::Error`] token is at least one byte, so a caller that goes on
    /// makes progress.
    pub fn next_token(&mut self) -> Token {
        // A tail call either way, so that lexing needs no frame here.
        if self.pos >= self.replay_from {
            return self.replay_or_lex();
        }
        self.lex()
    }

    /// The recorded token at `pos`, or the one lexed there.
    #[inline(never)]
    fn replay_or_lex(&mut self) -> Token {
        match self.replayed() {
            Some(token) => token,
            None => self.lex_out_of_line(),
        }
    }

    #[inline(never)]
    fn lex_out_of_line(&mut self) -> Token {
        self.lex()
    }

    /// The token at `pos`, lexed.
    #[allow(clippy::inline_always)] // Measured: `next_token` is this (WP-168).
    #[inline(always)]
    fn lex(&mut self) -> Token {
        let start = match self.skip_blank() {
            Ok(start) => start,
            Err(token) => return token,
        };
        let Some(&c) = self.src.get(start) else {
            if self.full.len() > MAX_SOURCE_BYTES {
                return self.fail("source is larger than 4 GiB", 0, 0);
            }
            return self.token(Tok::Eof, start, start);
        };
        let one = |kind| Token { kind, kw: Kw::Other, start: start as u32, end: start as u32 + 1 };
        // One dispatch on the first byte: the tokens that are most of a
        // text go straight to their rule, the rest to `compound`.
        let simple = match c {
            b'=' => Tok::Equal,
            b',' => Tok::Comma,
            b'*' => Tok::Star,
            b'[' => Tok::LBracket,
            b']' => Tok::RBracket,
            b'{' => Tok::LBrace,
            b'}' => Tok::RBrace,
            b'<' => Tok::Less,
            b'>' => Tok::Greater,
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b'|' => Tok::Bar,
            b':' => Tok::Colon,
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => return self.word(c, start),
            b'%' => return self.local(start),
            b'@' => return self.global(start),
            b'-' | b'0'..=b'9' => return self.number(c, start),
            _ => return self.compound(c, start),
        };
        self.pos = start + 1;
        one(simple)
    }

    #[inline(never)]
    fn local(&mut self, start: usize) -> Token {
        self.sigil(start, Tok::Local, Tok::LocalQuoted, Some(Tok::LocalId))
    }

    #[inline(never)]
    fn global(&mut self, start: usize) -> Token {
        self.sigil(start, Tok::Global, Tok::GlobalQuoted, Some(Tok::GlobalId))
    }

    /// Every first byte that `next_token` does not dispatch itself.
    #[inline(never)]
    fn compound(&mut self, c: u8, start: usize) -> Token {
        match c {
            b'@' => self.sigil(start, Tok::Global, Tok::GlobalQuoted, Some(Tok::GlobalId)),
            b'%' => self.sigil(start, Tok::Local, Tok::LocalQuoted, Some(Tok::LocalId)),
            b'$' => match self.label_tail(start) {
                Some(end) => self.token(Tok::Label, start, end),
                None => self.sigil(start, Tok::Comdat, Tok::ComdatQuoted, None),
            },
            b'"' => match self.string_end(start + 1) {
                Some(end) if self.at(end) == b':' => self.token(Tok::LabelQuoted, start, end + 1),
                Some(end) => self.token(Tok::Str, start, end),
                None => self.fail("unterminated string", start, start + 1),
            },
            b'!' => {
                let first = self.at(start + 1);
                if (is(first, LABEL) && !is(first, DIGIT)) || first == b'\\' {
                    let end = self.skip_while(start + 2, META);
                    self.token(Tok::Meta, start, end)
                } else {
                    self.token(Tok::Exclaim, start, start + 1)
                }
            }
            b'#' => {
                let first = self.at(start + 1);
                if is(first, DIGIT) {
                    let end = self.skip_while(start + 1, DIGIT);
                    self.token(Tok::AttrGroup, start, end)
                } else if is(first, ALPHA) || first == b'_' {
                    let end = self.skip_while(start + 1, WORD);
                    self.token(Tok::HashWord, start, end)
                } else {
                    self.token(Tok::Hash, start, start + 1)
                }
            }
            b'^' => {
                let end = self.skip_while(start + 1, DIGIT);
                if end == start + 1 {
                    return self.fail("expected a number after '^'", start, start + 1);
                }
                self.token(Tok::SummaryId, start, end)
            }
            b'.' => {
                if let Some(end) = self.label_tail(start) {
                    self.token(Tok::Label, start, end)
                } else if self.at(start + 1) == b'.' && self.at(start + 2) == b'.' {
                    self.token(Tok::DotDotDot, start, start + 3)
                } else {
                    self.fail("stray '.'", start, start + 1)
                }
            }
            b'+' => self.positive(start),
            b'-' | b'0'..=b'9' => self.number(c, start),
            _ if is(c, ALPHA) || c == b'_' => self.word(c, start),
            _ => self.fail("unexpected character", start, start + 1),
        }
    }

    /// The offset after the closing quote of a string whose contents start
    /// at `from`.
    fn string_end(&self, from: usize) -> Option<usize> {
        find_byte(self.src, from, b'"').map(|at| at + 1)
    }

    /// `@`, `%` or `$` and what follows.
    #[allow(clippy::inline_always)] // Measured: a call per name token otherwise (WP-159).
    #[inline(always)]
    fn sigil(&mut self, start: usize, name: Tok, quoted: Tok, id: Option<Tok>) -> Token {
        let first = self.at(start + 1);
        if first == b'"' {
            return match self.string_end(start + 2) {
                Some(end) => self.token(quoted, start, end),
                None => self.fail("unterminated quoted name", start, start + 2),
            };
        }
        if is(first, DIGIT) {
            let Some(id) = id else {
                return self.fail("expected a name", start, start + 1);
            };
            let end = self.skip_while(start + 1, DIGIT);
            return self.token(id, start, end);
        }
        if is(first, LABEL) {
            // For `$` the label test has just scanned this run.
            let end = if name == Tok::Comdat {
                self.label_run_end(start + 1)
            } else {
                self.skip_while(start + 1, LABEL)
            };
            return self.token(name, start, end);
        }
        self.fail("expected a name", start, start + 1)
    }

    /// `[0-9]*([eE][-+]?[0-9]+)?` after the point of a number.
    fn fraction_end(&self, from: usize) -> usize {
        let mut end = self.skip_while(from, DIGIT);
        if matches!(self.at(end), b'e' | b'E') {
            let sign = usize::from(matches!(self.at(end + 1), b'-' | b'+'));
            if is(self.at(end + 1 + sign), DIGIT) {
                end = self.skip_while(end + 1 + sign, DIGIT);
            }
        }
        end
    }

    /// `+[0-9]+.…`: a floating-point number with an explicit plus.
    fn positive(&mut self, start: usize) -> Token {
        let digits = self.skip_while(start + 1, DIGIT);
        if digits == start + 1 || self.at(digits) != b'.' {
            return self.fail("expected a floating-point number after '+'", start, start + 1);
        }
        let end = self.fraction_end(digits + 1);
        self.token(Tok::Float, start, end)
    }

    /// A token that starts with a digit or `-`: a label, an integer, a
    /// floating-point number or a hexadecimal form.
    ///
    /// Most are digits, with or without a `-`, followed by a byte that
    /// neither goes on a label nor ends one, which [`Lexer::number_rule`]
    /// makes an integer whatever the digits: that is looked at first.
    #[inline(never)]
    fn number(&mut self, c: u8, start: usize) -> Token {
        if c != b'-' || is(self.at(start + 1), DIGIT) {
            let digits = self.skip_while(start + 1, DIGIT);
            let next = self.at(digits);
            if !is(next, LABEL) && next != b':' {
                return self.token(Tok::Int, start, digits);
            }
        }
        self.number_rule(c, start)
    }

    /// The rules for a token that starts with a digit or `-`.
    #[inline(never)]
    fn number_rule(&mut self, c: u8, start: usize) -> Token {
        if c == b'-' && !is(self.at(start + 1), DIGIT) {
            return match self.label_tail(start) {
                Some(end) => self.token(Tok::Label, start, end),
                None => self.fail("stray '-'", start, start + 1),
            };
        }
        let digits = self.skip_while(start + 1, DIGIT);
        if c != b'-' && self.at(digits) == b':' {
            return self.token(Tok::LabelId, start, digits + 1);
        }
        if (is(self.at(digits), LABEL) || self.at(digits) == b':')
            && let Some(end) = self.label_tail(digits)
        {
            return self.token(Tok::Label, start, end);
        }
        if self.at(digits) != b'.' {
            if c == b'0' && self.at(start + 1) == b'x' {
                return self.hex_float(start);
            }
            return self.token(Tok::Int, start, digits);
        }
        let end = self.fraction_end(digits + 1);
        self.token(Tok::Float, start, end)
    }

    /// `0x`, an optional kind letter, hexadecimal digits.
    fn hex_float(&mut self, start: usize) -> Token {
        let (kind, digits) = match self.at(start + 2) {
            b'K' => (Tok::HexFp80, start + 3),
            b'L' => (Tok::HexFp128, start + 3),
            b'M' => (Tok::HexPpcFp128, start + 3),
            b'H' => (Tok::HexHalf, start + 3),
            b'R' => (Tok::HexBFloat, start + 3),
            _ => (Tok::HexDouble, start + 2),
        };
        let end = self.skip_while(digits, HEX);
        if end == digits {
            return self.fail("expected hexadecimal digits after '0x'", start, start + 1);
        }
        self.token(kind, start, end)
    }

    /// A token that starts with a letter or `_`. Most are a word or an
    /// integer type (`i` and digits) of at most 15 bytes with sixteen
    /// bytes of source from its start, not `u0…`, `s0…` or `cc` and a
    /// digit, and followed by a byte that neither goes on a label nor
    /// ends one: for those the rules of [`Lexer::word_rule`] all come to
    /// the word or the type itself, which is found, and its key made,
    /// from those sixteen bytes. Everything else goes through the rules.
    #[inline(never)]
    fn word(&mut self, c: u8, start: usize) -> Token {
        let Some(bytes) = self.src.get(start..).and_then(<[u8]>::first_chunk::<16>) else {
            return self.word_rule(c, start);
        };
        let second = bytes[1];
        let int_type = c == b'i' && is(second, DIGIT);
        let special = (second == b'0' && matches!(c, b'u' | b's'))
            || (c == b'c' && second == b'c' && is(bytes[2], DIGIT));
        let rest = &bytes[1..];
        let len = if int_type {
            rest.iter().position(|&b| !is(b, DIGIT))
        } else {
            rest.iter().position(|&b| !is(b, WORD))
        }
        .map_or(16, |n| n + 1);
        if special || len == 16 || is(bytes[len], LABEL) || bytes[len] == b':' {
            return self.word_rule(c, start);
        }
        let (first, rest) = bytes.split_at(8);
        let lo = u64::from_le_bytes(first.try_into().unwrap_or_default());
        let hi = u64::from_le_bytes(rest.try_into().unwrap_or_default());
        let key = if len <= 8 {
            [lo & (u64::MAX >> (64 - 8 * len)), 0]
        } else {
            [lo, hi & (u64::MAX >> (128 - 8 * len))]
        };
        self.pos = start + len;
        let kind = if int_type { Tok::IntType } else { Tok::Word };
        Token { kind, kw: by_key(key), start: start as u32, end: (start + len) as u32 }
    }

    /// A token that starts with a letter or `_`: a label, an integer
    /// type, a hexadecimal integer or a word.
    #[inline(never)]
    fn word_rule(&mut self, c: u8, start: usize) -> Token {
        // `i` and digits: an integer type unless the whole run of label
        // characters ends in a colon.
        if c == b'i' && is(self.at(start + 1), DIGIT) {
            let digits = self.skip_while(start + 1, DIGIT);
            return match self.label_tail(digits) {
                Some(end) => self.token(Tok::Label, start, end),
                None => self.keyword_token(Tok::IntType, start, digits),
            };
        }
        let end = self.skip_while(start + 1, WORD);
        if let Some(label) = self.label_tail(end) {
            return self.token(Tok::Label, start, label);
        }
        let text = &self.src[start..end];
        if text.len() > 3
            && matches!(c, b'u' | b's')
            && text[1] == b'0'
            && text[2] == b'x'
            && text[3..].iter().all(u8::is_ascii_hexdigit)
        {
            return self.token(Tok::HexInt, start, end);
        }
        // `cc42` is `cc` and `42`.
        if text.len() > 2 && text.starts_with(b"cc") && text[2].is_ascii_digit() {
            return self.keyword_token(Tok::Word, start, start + 2);
        }
        self.keyword_token(Tok::Word, start, end)
    }
}

/// Appends to `out` the bytes that the contents of a string or quoted
/// name stand for: `\\` is a backslash, `\xx` with two hexadecimal digits
/// is that byte, and any other backslash is itself.
pub fn unescape(raw: &[u8], out: &mut Vec<u8>) {
    let mut rest = raw;
    while let Some(at) = rest.iter().position(|&b| b == b'\\') {
        out.extend_from_slice(&rest[..at]);
        let after = &rest[at + 1..];
        match after {
            [b'\\', ..] => {
                out.push(b'\\');
                rest = &after[1..];
            }
            [hi, lo, ..] if hi.is_ascii_hexdigit() && lo.is_ascii_hexdigit() => {
                out.push(hex_value(*hi) << 4 | hex_value(*lo));
                rest = &after[2..];
            }
            _ => {
                out.push(b'\\');
                rest = after;
            }
        }
    }
    out.extend_from_slice(rest);
}

/// The value of a hexadecimal digit.
pub(crate) fn hex_value(digit: u8) -> u8 {
    match digit {
        b'0'..=b'9' => digit - b'0',
        b'a'..=b'f' => digit - b'a' + 10,
        _ => digit.wrapping_sub(b'A').wrapping_add(10) & 15,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(text: &str) -> Vec<(Tok, &str)> {
        let mut lexer = Lexer::new(text.as_bytes());
        let mut out = Vec::new();
        loop {
            let token = lexer.next_token();
            if token.kind == Tok::Eof {
                return out;
            }
            out.push((token.kind, &text[token.start as usize..token.end as usize]));
            assert!(token.end > token.start, "empty token in {text:?}");
        }
    }

    fn kinds(text: &str) -> Vec<Tok> {
        lex(text).into_iter().map(|(kind, _)| kind).collect()
    }

    /// §2.1: every sigil form, named, quoted and numbered.
    #[test]
    fn names() {
        assert_eq!(
            lex("@a.b-c$d @\"x y\" @12 %s %\"q\" %0 $c $\"d\" !dbg !llvm.module.flags #3 ^7"),
            [
                (Tok::Global, "@a.b-c$d"),
                (Tok::GlobalQuoted, "@\"x y\""),
                (Tok::GlobalId, "@12"),
                (Tok::Local, "%s"),
                (Tok::LocalQuoted, "%\"q\""),
                (Tok::LocalId, "%0"),
                (Tok::Comdat, "$c"),
                (Tok::ComdatQuoted, "$\"d\""),
                (Tok::Meta, "!dbg"),
                (Tok::Meta, "!llvm.module.flags"),
                (Tok::AttrGroup, "#3"),
                (Tok::SummaryId, "^7"),
            ]
        );
        // `%-1` is a name, and a quote inside a name is written `\22`:
        // the name ends at the first `"`.
        assert_eq!(
            lex("%-1 @\"a\\22b\" x"),
            [(Tok::Local, "%-1"), (Tok::GlobalQuoted, "@\"a\\22b\""), (Tok::Word, "x")]
        );
        assert_eq!(kinds("@ x"), [Tok::Error, Tok::Word]);
        assert_eq!(kinds("$1"), [Tok::Error, Tok::Int]);
        assert_eq!(kinds("@\"abc"), [Tok::Error, Tok::Word]);
    }

    /// §2.1: metadata tokens and debug-record heads.
    #[test]
    fn metadata_and_hash() {
        assert_eq!(
            lex("!0 !{!\"s\"} !DILocation(line: 1) !a\\20b #dbg_value( # 1"),
            [
                (Tok::Exclaim, "!"),
                (Tok::Int, "0"),
                (Tok::Exclaim, "!"),
                (Tok::LBrace, "{"),
                (Tok::Exclaim, "!"),
                (Tok::Str, "\"s\""),
                (Tok::RBrace, "}"),
                (Tok::Meta, "!DILocation"),
                (Tok::LParen, "("),
                (Tok::Label, "line:"),
                (Tok::Int, "1"),
                (Tok::RParen, ")"),
                (Tok::Meta, "!a\\20b"),
                (Tok::HashWord, "#dbg_value"),
                (Tok::LParen, "("),
                (Tok::Hash, "#"),
                (Tok::Int, "1"),
            ]
        );
    }

    /// §2.2: words, integer types, and where a word ends.
    #[test]
    fn words() {
        assert_eq!(
            lex("define i32 i8388607 i1x x86_fp80 x c\"a\" cc10 cc 11 ccc DW_TAG_member a.b"),
            [
                (Tok::Word, "define"),
                (Tok::IntType, "i32"),
                (Tok::IntType, "i8388607"),
                (Tok::IntType, "i1"),
                (Tok::Word, "x"),
                (Tok::Word, "x86_fp80"),
                (Tok::Word, "x"),
                (Tok::Word, "c"),
                (Tok::Str, "\"a\""),
                (Tok::Word, "cc"),
                (Tok::Int, "10"),
                (Tok::Word, "cc"),
                (Tok::Int, "11"),
                (Tok::Word, "ccc"),
                (Tok::Word, "DW_TAG_member"),
                (Tok::Word, "a"),
                (Tok::Error, "."),
                (Tok::Word, "b"),
            ]
        );
    }

    /// §2.3: labels win over everything a run of label characters could
    /// otherwise be.
    #[test]
    fn labels() {
        assert_eq!(
            lex("entry: 12: 1x: -1: i32: a.b-c: \"q r\": $x: .L: 0x1F: 1.5:"),
            [
                (Tok::Label, "entry:"),
                (Tok::LabelId, "12:"),
                (Tok::Label, "1x:"),
                (Tok::Label, "-1:"),
                (Tok::Label, "i32:"),
                (Tok::Label, "a.b-c:"),
                (Tok::LabelQuoted, "\"q r\":"),
                (Tok::Label, "$x:"),
                (Tok::Label, ".L:"),
                (Tok::Label, "0x1F:"),
                (Tok::Label, "1.5:"),
            ]
        );
        // A colon after white space is its own token.
        assert_eq!(kinds("a :"), [Tok::Word, Tok::Colon]);
    }

    /// §2.4: integers and every floating-point form.
    #[test]
    fn numbers() {
        assert_eq!(
            lex("0 -7 123456789012345678901234567890 u0x1F s0xff 1.0 -0.0 +2.5 1.5e+10 2.e3 3.5e"),
            [
                (Tok::Int, "0"),
                (Tok::Int, "-7"),
                (Tok::Int, "123456789012345678901234567890"),
                (Tok::HexInt, "u0x1F"),
                (Tok::HexInt, "s0xff"),
                (Tok::Float, "1.0"),
                (Tok::Float, "-0.0"),
                (Tok::Float, "+2.5"),
                (Tok::Float, "1.5e+10"),
                (Tok::Float, "2.e3"),
                (Tok::Float, "3.5"),
                (Tok::Word, "e"),
            ]
        );
        assert_eq!(
            lex("0x3FF0000000000000 0xK3FFF8000000000000000 0xL00000000000000003FFF000000000000 \
                 0xM0000000000000000 0xH3C00 0xR3F80"),
            [
                (Tok::HexDouble, "0x3FF0000000000000"),
                (Tok::HexFp80, "0xK3FFF8000000000000000"),
                (Tok::HexFp128, "0xL00000000000000003FFF000000000000"),
                (Tok::HexPpcFp128, "0xM0000000000000000"),
                (Tok::HexHalf, "0xH3C00"),
                (Tok::HexBFloat, "0xR3F80"),
            ]
        );
        assert_eq!(kinds("0xZ"), [Tok::Error, Tok::Word]);
        assert_eq!(kinds("+1"), [Tok::Error, Tok::Int]);
        assert_eq!(kinds("- 1"), [Tok::Error, Tok::Int]);
        // A negative number is not hexadecimal.
        assert_eq!(lex("-0x10"), [(Tok::Int, "-0"), (Tok::Word, "x10")]);
    }

    /// §2.5: strings end at the first quote; punctuation.
    #[test]
    fn strings_and_punctuation() {
        assert_eq!(
            lex("\"a\\22b\" \"\" \"}\""),
            [(Tok::Str, "\"a\\22b\""), (Tok::Str, "\"\""), (Tok::Str, "\"}\"")]
        );
        assert_eq!(
            kinds("= , * [ ] { } < > ( ) | : ..."),
            [
                Tok::Equal,
                Tok::Comma,
                Tok::Star,
                Tok::LBracket,
                Tok::RBracket,
                Tok::LBrace,
                Tok::RBrace,
                Tok::Less,
                Tok::Greater,
                Tok::LParen,
                Tok::RParen,
                Tok::Bar,
                Tok::Colon,
                Tok::DotDotDot,
            ]
        );
        assert_eq!(kinds("\"abc"), [Tok::Error, Tok::Word]);
        assert_eq!(kinds(".."), [Tok::Error, Tok::Error]);
        assert_eq!(kinds("\x0c ~ `"), [Tok::Error, Tok::Error, Tok::Error]);
    }

    /// §2.6: comments, and a zero byte as white space.
    #[test]
    fn blanks() {
        assert_eq!(kinds("a ; b \"c\n d /* e ; \" */ f\0g"), [Tok::Word; 4]);
        assert_eq!(kinds("a ; no newline"), [Tok::Word]);
        assert_eq!(kinds("a /* open"), [Tok::Word, Tok::Error, Tok::Word]);
        assert_eq!(kinds("a / b"), [Tok::Word, Tok::Error, Tok::Word]);
        let mut lexer = Lexer::new(b"  ");
        assert_eq!(lexer.next_token(), Token { kind: Tok::Eof, kw: Kw::Other, start: 2, end: 2 });
        assert_eq!(lexer.next_token().kind, Tok::Eof);
    }

    /// WP-096: white space and metadata names are skipped by class, a
    /// table lookup per byte, and a token is looked for after one byte's
    /// test when no blank or comment can start; the classes are exactly
    /// the sets §2 names, for every byte value.
    #[test]
    fn classes_are_the_sets_of_section_2() {
        for b in 0..=255u8 {
            let label = b.is_ascii_alphanumeric() || b"-$._".contains(&b);
            assert_eq!(is(b, BLANK), b" \t\n\r\0".contains(&b), "BLANK {b:#04x}");
            assert_eq!(is(b, META), label || b == b'\\', "META {b:#04x}");
            assert_eq!(is(b, LABEL), label, "LABEL {b:#04x}");
            assert_eq!(is(b, SKIP), b" \t\n\r\0;/".contains(&b), "SKIP {b:#04x}");
        }
        assert_eq!(
            lex("!a\\b-c.d$e_9 \0\r\t\n!x"),
            [(Tok::Meta, "!a\\b-c.d$e_9"), (Tok::Meta, "!x")]
        );
    }

    /// The word fast path is the word rules: every word made of the
    /// bytes the rules look at, of every length around the sixteen bytes
    /// the fast path reads, before every kind of byte, agrees with
    /// `word_rule` in its token and in where it leaves the lexer.
    #[test]
    fn the_word_fast_path_is_the_rules() {
        let heads: [&[u8]; 12] =
            [b"x", b"i", b"i3", b"u0", b"s0x1f", b"u0xfg", b"cc", b"cc1", b"c", b"ret", b"_", b"Z"];
        let fill = b"a0_9Zq";
        let tails: [&[u8]; 12] =
            [b"", b" ", b":", b".", b"-", b"$", b",", b"(", b"\"", b"\\", b"\x80", b"a:"];
        let mut cases = 0;
        for head in heads {
            for len in 0..20 {
                for tail in tails {
                    for pad in [0usize, 1, 16, 32] {
                        let mut text = head.to_vec();
                        text.extend((0..len).map(|n| fill[n % fill.len()]));
                        text.extend_from_slice(tail);
                        text.extend(std::iter::repeat_n(b' ', pad));
                        let mut fast = Lexer::new(&text);
                        let mut rule = Lexer::new(&text);
                        let (a, b) = (fast.word(text[0], 0), rule.word_rule(text[0], 0));
                        assert_eq!(
                            (a, fast.pos()),
                            (b, rule.pos()),
                            "{:?}",
                            String::from_utf8_lossy(&text)
                        );
                        cases += 1;
                    }
                }
            }
        }
        // And every keyword, as itself and with a byte more.
        for &word in &crate::kw::TEXTS[1..] {
            for tail in [&b" "[..], b"x ", b"  "] {
                let text = [word, tail, &[b' '; 16]].concat();
                let mut fast = Lexer::new(&text);
                let mut rule = Lexer::new(&text);
                if text[0].is_ascii_alphabetic() || text[0] == b'_' {
                    assert_eq!(fast.word(text[0], 0), rule.word_rule(text[0], 0));
                    cases += 1;
                }
            }
        }
        assert!(cases > 10_000);
    }

    /// §2.8: a lexer that replays the tokens another lexer recorded from
    /// some offsets on gives the tokens it would have lexed, whatever
    /// the recording skipped and wherever the replaying lexer moves.
    #[test]
    fn replayed_tokens_are_the_lexed_ones() {
        let text = b"entry:\n  %1 = call i32 @f(i32 %x, ptr \"q\") ; c\n  br label %next\n\
                     next: ; preds\n  %y = add nuw i32 %1, -7\n  ret i32 %y\n}\n";
        let plain: Vec<(usize, Token)> = {
            let mut lexer = Lexer::new(text);
            std::iter::from_fn(|| {
                let from = lexer.pos();
                let token = lexer.next_token();
                (token.kind != Tok::Eof).then_some((from, token))
            })
            .collect()
        };
        // Record every other token from the third on, as a scan that skips
        // some does.
        for skip in 1..4 {
            let mut scan = Lexer::new(text);
            let mut recording = Recording::default();
            for (n, &(from, _)) in plain.iter().enumerate().skip(2) {
                scan.set_pos(from);
                let token = scan.next_token();
                if n % skip == 0 {
                    recording.push(from, token);
                }
            }
            let mut lexer = Lexer::new(text);
            lexer.replay(recording);
            for &(from, token) in &plain {
                assert_eq!(lexer.pos(), from);
                assert_eq!(lexer.next_token(), token);
            }
            assert_eq!(lexer.next_token().kind, Tok::Eof);
            // Moving back and forth: every token is still the one lexed.
            for &(from, token) in plain.iter().rev().chain(plain.iter().step_by(3)) {
                lexer.set_pos(from);
                assert_eq!(lexer.next_token(), token);
            }
        }
    }

    /// §2.7: escapes.
    #[test]
    fn unescaping() {
        let mut out = Vec::new();
        unescape(br"a\22b\\c\0Ad\zz\4", &mut out);
        assert_eq!(out, b"a\"b\\c\nd\\zz\\4");
    }

    /// §8: a long run of label characters is looked at a bounded number
    /// of times, however many tokens it makes.
    #[test]
    fn label_runs_are_linear() {
        let text = "-1".repeat(200_000) + " " + &"i1".repeat(200_000) + " " + &"$a".repeat(100_000);
        let mut lexer = Lexer::new(text.as_bytes());
        let mut count = 0;
        while lexer.next_token().kind != Tok::Eof {
            count += 1;
        }
        assert_eq!(count, 200_000 + 200_000 + 1);
    }
}
