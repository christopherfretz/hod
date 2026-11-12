//! Constants (`spec/ll.md` §5.3 to §5.5): the whole constant syntax of
//! LLVM 19 to 22, built through `hod_ir`'s folding functions so that an
//! address always arrives in its normal form.

use std::collections::HashMap;

use hod_ir::{BlockId, ConstId, ConstOp, SymId, TypeId, TypeKind};

use crate::error::{LlErrorKind, PResult, fail};
use crate::float::{Narrow, decimal, fp80, fp128, hex_u64, hex_value_u64, narrow};
use crate::index::{ItemKind, parse_u32};
use crate::lexer::{Lexer, Tok, Token, hex_value, unescape};
use crate::limits::MAX_INT_LITERAL_DIGITS;
use crate::parser::{Parser, ir};

impl Parser<'_, '_> {
    /// `type value`.
    pub(crate) fn parse_typed_const(&mut self) -> PResult<ConstId> {
        let ty = self.parse_type()?;
        self.parse_const(ty)
    }

    /// A constant of type `ty`.
    pub(crate) fn parse_const(&mut self, ty: TypeId) -> PResult<ConstId> {
        let token = self.tok;
        let at = token.start;
        match token.kind {
            Tok::Int => {
                self.bump();
                self.int_literal(ty, token)
            }
            Tok::HexInt => {
                self.bump();
                self.hex_int_literal(ty, token)
            }
            Tok::Float
            | Tok::HexDouble
            | Tok::HexFp80
            | Tok::HexFp128
            | Tok::HexPpcFp128
            | Tok::HexHalf
            | Tok::HexBFloat => {
                self.bump();
                let bits = self.float_bits(ty, token)?;
                ir(at, self.module.consts.float(&self.module.types, ty, bits))
            }
            Tok::Global | Tok::GlobalQuoted | Tok::GlobalId => {
                let sym = self.global_ref(token)?;
                self.bump();
                self.address_of(sym, ty, at)
            }
            Tok::LBrace => {
                self.packing(ty, false)?;
                self.aggregate(ty, Tok::RBrace)
            }
            Tok::LBracket => self.aggregate(ty, Tok::RBracket),
            Tok::Less => {
                self.bump();
                if self.tok.kind == Tok::LBrace {
                    self.packing(ty, true)?;
                    let id = self.aggregate(ty, Tok::RBrace)?;
                    self.expect(Tok::Greater, "expected '>' at the end of a packed struct")?;
                    return Ok(id);
                }
                self.elements(ty, Tok::Greater, at)
            }
            Tok::Word => self.word_const(ty),
            _ => self.syntax("expected a value"),
        }
    }

    /// Checks that a struct constant written packed or not (`<{ … }>` or
    /// `{ … }`) is given a struct type that is the same.
    fn packing(&mut self, ty: TypeId, written: bool) -> PResult<()> {
        match self.module.types.kind(ty) {
            TypeKind::Struct { packed, .. } if packed != written => fail(
                LlErrorKind::Type,
                self.tok.start,
                "the initializer and its type are not both packed",
            ),
            _ => Ok(()),
        }
    }

    /// `{ … }` or `[ … ]`, at the opening bracket.
    fn aggregate(&mut self, ty: TypeId, close: Tok) -> PResult<ConstId> {
        let at = self.tok.start;
        self.bump();
        self.elements(ty, close, at)
    }

    /// The elements of an aggregate up to and including `close`.
    fn elements(&mut self, ty: TypeId, close: Tok, at: u32) -> PResult<ConstId> {
        let fits = match self.module.types.kind(ty) {
            TypeKind::Struct { .. } => close == Tok::RBrace,
            TypeKind::Array { .. } => close == Tok::RBracket,
            TypeKind::Vector { .. } => close == Tok::Greater,
            _ => false,
        };
        if !fits {
            return fail(
                LlErrorKind::Type,
                at,
                "this aggregate does not have the type it is given",
            );
        }
        self.enter()?;
        let base = self.elems.len();
        if self.tok.kind != close {
            loop {
                let elem = self.parse_typed_const()?;
                self.elems.push(elem);
                if !self.eat(Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(close, "expected ',' or the end of the aggregate")?;
        let id = self.module.consts.aggregate(&self.module.types, ty, &self.elems[base..]);
        self.elems.truncate(base);
        self.leave();
        ir(at, id)
    }

    /// The address of `sym` as a constant of the pointer type `ty`.
    fn address_of(&mut self, sym: SymId, ty: TypeId, at: u32) -> PResult<ConstId> {
        let address = ir(at, self.module.consts.addr(sym, 0))?;
        if ty == TypeId::PTR {
            return Ok(address);
        }
        if !self.module.types.is_ptr(ty) {
            return fail(LlErrorKind::Type, at, "a global's address must have a pointer type");
        }
        // A pointer in another address space: `hod_ir` has addresses in
        // space 0 only, so this is the cast it would be.
        let types = &self.module.types;
        ir(at, self.module.consts.fold_cast(types, ConstOp::AddrSpaceCast, address, ty))
    }

    /// A decimal integer.
    fn int_literal(&mut self, ty: TypeId, token: Token) -> PResult<ConstId> {
        let at = token.start;
        let text = self.tok_text(token);
        let Some(bits) = self.module.types.int_bits(ty) else {
            return fail(LlErrorKind::Type, at, "an integer constant must have an integer type");
        };
        let (negative, digits) = match text.strip_prefix(b"-") {
            Some(digits) => (true, digits),
            None => (false, text),
        };
        let (consts, types) = (&mut self.module.consts, &self.module.types);
        if digits.len() <= 18 {
            // Fits 63 bits: no overflow in either direction.
            let value = digits.iter().fold(0u64, |v, &d| v * 10 + u64::from(d - b'0'));
            return ir(
                at,
                if negative {
                    consts.int_signed(types, ty, -(value as i64))
                } else {
                    consts.int(types, ty, value)
                },
            );
        }
        if digits.len() > MAX_INT_LITERAL_DIGITS {
            return fail(
                LlErrorKind::Limit,
                at,
                "an integer literal has more than MAX_INT_LITERAL_DIGITS digits",
            );
        }
        // The value modulo 2^(64 * words), which is all the type keeps.
        let words = &mut self.words;
        words.clear();
        words.resize((bits as usize).div_ceil(64), 0);
        let mut used = 1;
        for chunk in digits.chunks(18) {
            let scale = 10u64.pow(chunk.len() as u32);
            let mut carry = chunk.iter().fold(0u64, |v, &d| v * 10 + u64::from(d - b'0'));
            for word in &mut words[..used] {
                let wide = u128::from(*word) * u128::from(scale) + u128::from(carry);
                *word = wide as u64;
                carry = (wide >> 64) as u64;
            }
            if carry != 0 && used < words.len() {
                words[used] = carry;
                used += 1;
            }
        }
        if negative {
            let mut carry = true;
            for word in words.iter_mut() {
                let (negated, overflow) = (!*word).overflowing_add(u64::from(carry));
                *word = negated;
                carry = overflow;
            }
        }
        ir(at, consts.int_from_words(types, ty, words))
    }

    /// `u0x…` or `s0x…`: the digits are the value's bits; with `s`, the
    /// highest set bit is the sign.
    fn hex_int_literal(&mut self, ty: TypeId, token: Token) -> PResult<ConstId> {
        let at = token.start;
        let text = self.tok_text(token);
        let Some(bits) = self.module.types.int_bits(ty) else {
            return fail(LlErrorKind::Type, at, "an integer constant must have an integer type");
        };
        let digits = &text[3..];
        let words = &mut self.words;
        words.clear();
        words.resize((bits as usize).div_ceil(64).max(1), 0);
        let mut active = 0usize;
        for (place, &digit) in digits.iter().rev().enumerate() {
            let value = u64::from(hex_value(digit));
            if value != 0 {
                active = place * 4 + (64 - value.leading_zeros() as usize);
            }
            if let Some(word) = words.get_mut(place / 16) {
                *word |= value << (place % 16 * 4);
            }
        }
        if text[0] == b's' && active > 0 {
            // Sign-extend from the highest set bit: every bit above it
            // becomes one.
            for (nth, word) in words.iter_mut().enumerate() {
                let low = nth * 64;
                if active >= low + 64 {
                    continue;
                }
                *word |= if active <= low { u64::MAX } else { u64::MAX << (active - low) };
            }
        }
        ir(at, self.module.consts.int_from_words(&self.module.types, ty, words))
    }

    /// The bit pattern of a floating-point literal in the type `ty`.
    fn float_bits(&mut self, ty: TypeId, token: Token) -> PResult<u128> {
        let at = token.start;
        let text = self.tok_text(token);
        let kind = self.module.types.kind(ty);
        let narrow_to = match kind {
            TypeKind::Half => Some(Narrow::Half),
            TypeKind::BFloat => Some(Narrow::BFloat),
            TypeKind::Float => Some(Narrow::Float),
            TypeKind::Double | TypeKind::Fp128 | TypeKind::X86Fp80 => None,
            _ => {
                return fail(
                    LlErrorKind::Type,
                    at,
                    "a floating-point constant must have a floating-point type",
                );
            }
        };
        let too_wide = "a hexadecimal floating-point constant has too many digits";
        let wrong = "this form of floating-point constant does not have the type it is given";
        let wide = |bits: Option<u128>, fits: bool| match bits {
            Some(bits) if fits => Ok(bits),
            Some(_) => fail(LlErrorKind::Type, at, wrong),
            None => fail(LlErrorKind::Lex, at, too_wide),
        };
        let as_double = match token.kind {
            Tok::Float => decimal(text),
            Tok::HexDouble => hex_value_u64(&text[2..]),
            Tok::HexFp80 => return wide(fp80(&text[3..]), kind == TypeKind::X86Fp80),
            // `ppc_fp128` is stood in for by `fp128` (and noted), so its
            // constants land here too.
            Tok::HexFp128 | Tok::HexPpcFp128 => {
                return wide(fp128(&text[3..]), kind == TypeKind::Fp128);
            }
            Tok::HexHalf => {
                let bits = hex_u64(&text[3..]).map(|bits| u128::from(bits & 0xffff));
                return wide(bits, kind == TypeKind::Half);
            }
            _ => {
                let bits = hex_u64(&text[3..]).map(|bits| u128::from(bits & 0xffff));
                return wide(bits, kind == TypeKind::BFloat);
            }
        };
        let Some(double) = as_double else {
            return fail(LlErrorKind::Lex, at, too_wide);
        };
        match (kind, narrow_to) {
            (TypeKind::Double, _) => Ok(u128::from(double)),
            (_, Some(to)) => match narrow(double, to) {
                Some(bits) => Ok(u128::from(bits)),
                None => fail(
                    LlErrorKind::Type,
                    at,
                    "this floating-point constant is not exactly a value of its type",
                ),
            },
            _ => fail(LlErrorKind::Type, at, wrong),
        }
    }

    /// A constant that starts with a word.
    fn word_const(&mut self, ty: TypeId) -> PResult<ConstId> {
        let at = self.tok.start;
        let word = self.text();
        let (consts, types) = (&mut self.module.consts, &self.module.types);
        let simple = match word {
            b"true" | b"false" if ty == TypeId::I1 => {
                consts.int(types, ty, u64::from(word == b"true"))
            }
            b"true" | b"false" => {
                return fail(LlErrorKind::Type, at, "a boolean constant must have type i1");
            }
            b"null" => consts.null(types, ty),
            b"zeroinitializer" => consts.zero(types, ty),
            b"undef" => consts.undef(types, ty),
            b"poison" => consts.poison(types, ty),
            b"none" if ty == TypeId::TOKEN => consts.none_token(),
            _ => {
                let id = self.expression(ty)?;
                if self.module.consts.ty(id) != ty {
                    return fail(
                        LlErrorKind::Type,
                        at,
                        "this constant does not have the type it is given",
                    );
                }
                return Ok(id);
            }
        };
        self.bump();
        ir(at, simple)
    }

    /// A string or a constant expression, at its first word.
    fn expression(&mut self, ty: TypeId) -> PResult<ConstId> {
        let at = self.tok.start;
        let word = self.text();
        let cast = |op| Some((op, false));
        let binary = |op| Some((op, true));
        let operator = match word {
            b"trunc" => cast(ConstOp::Trunc),
            b"ptrtoint" => cast(ConstOp::PtrToInt),
            b"ptrtoaddr" => cast(ConstOp::PtrToAddr),
            b"inttoptr" => cast(ConstOp::IntToPtr),
            b"bitcast" => cast(ConstOp::BitCast),
            b"addrspacecast" => cast(ConstOp::AddrSpaceCast),
            b"add" => binary(ConstOp::Add),
            b"sub" => binary(ConstOp::Sub),
            b"mul" => binary(ConstOp::Mul),
            b"xor" => binary(ConstOp::Xor),
            _ => None,
        };
        self.enter()?;
        self.bump();
        let id = match (word, operator) {
            (_, Some((op, false))) => {
                self.expect(Tok::LParen, "expected '(' after a cast")?;
                let value = self.parse_typed_const()?;
                self.expect_word(b"to", "expected 'to' in a cast")?;
                let to = self.parse_type()?;
                self.expect(Tok::RParen, "expected ')' at the end of a cast")?;
                ir(at, self.module.consts.fold_cast(&self.module.types, op, value, to))?
            }
            (_, Some((op, true))) => {
                while self.at_word(b"nuw") || self.at_word(b"nsw") {
                    self.bump();
                }
                self.expect(Tok::LParen, "expected '(' after a binary operator")?;
                let lhs = self.parse_typed_const()?;
                self.expect(Tok::Comma, "expected ',' between operands")?;
                let rhs = self.parse_typed_const()?;
                self.expect(Tok::RParen, "expected ')' after the operands")?;
                ir(at, self.module.consts.fold_binary(&self.module.types, op, lhs, rhs))?
            }
            (b"c", _) => {
                let text = self.expect(Tok::Str, "expected a string after 'c'")?;
                self.buf.clear();
                let src = self.src;
                unescape(&src[text.start as usize + 1..text.end as usize - 1], &mut self.buf);
                ir(at, self.module.consts.bytes(&mut self.module.types, &self.buf))?
            }
            (b"getelementptr", _) => self.gep(at)?,
            (b"extractelement", _) => self.vector_op(ConstOp::ExtractElement, ty, at)?,
            (b"insertelement", _) => self.vector_op(ConstOp::InsertElement, ty, at)?,
            (b"shufflevector", _) => self.vector_op(ConstOp::ShuffleVector, ty, at)?,
            (b"blockaddress", _) => self.block_address(at)?,
            (b"dso_local_equivalent", _) => {
                let sym = self.global_ref(self.tok)?;
                self.bump();
                ir(at, self.module.consts.dso_local_equivalent(sym))?
            }
            (b"no_cfi", _) => {
                let sym = self.global_ref(self.tok)?;
                self.bump();
                ir(at, self.module.consts.no_cfi(sym))?
            }
            (b"splat", _) => {
                self.expect(Tok::LParen, "expected '(' after splat")?;
                let elem = self.parse_typed_const()?;
                self.expect(Tok::RParen, "expected ')' after the splat value")?;
                ir(at, self.module.consts.splat(&self.module.types, ty, elem))?
            }
            (b"ptrauth", _) => {
                // A signed pointer: `hod_ir` has no form for it. Noted,
                // and the pointer stands in for it.
                self.note("const-expr:ptrauth");
                self.expect(Tok::LParen, "expected '(' after ptrauth")?;
                let pointer = self.parse_typed_const()?;
                while self.eat(Tok::Comma) {
                    self.parse_typed_const()?;
                }
                self.expect(Tok::RParen, "expected ')' after the ptrauth operands")?;
                pointer
            }
            _ => return fail(LlErrorKind::Syntax, at, "expected a value"),
        };
        self.leave();
        Ok(id)
    }

    /// `getelementptr flags (type, base, indices…)`, after the keyword.
    fn gep(&mut self, at: u32) -> PResult<ConstId> {
        loop {
            if self.at_word(b"inrange") {
                self.bump();
                if self.tok.kind != Tok::LParen {
                    return self.syntax("expected '(' after inrange");
                }
                self.skip_brackets()?;
            } else if self.tok.kind == Tok::Word
                && matches!(self.text(), b"inbounds" | b"nuw" | b"nusw")
            {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(Tok::LParen, "expected '(' after getelementptr")?;
        let source = self.parse_type()?;
        self.expect(Tok::Comma, "expected ',' after the source element type")?;
        let base = self.parse_typed_const()?;
        let first = self.elems.len();
        while self.eat(Tok::Comma) {
            let index = self.parse_typed_const()?;
            self.elems.push(index);
        }
        self.expect(Tok::RParen, "expected ')' at the end of a getelementptr")?;
        let module = &mut self.module;
        let id = module.consts.fold_gep(&mut module.types, source, base, &self.elems[first..]);
        self.elems.truncate(first);
        ir(at, id)
    }

    /// `extractelement`, `insertelement` or `shufflevector`: kept as an
    /// expression of the type the context gives.
    fn vector_op(&mut self, op: ConstOp, ty: TypeId, at: u32) -> PResult<ConstId> {
        self.expect(Tok::LParen, "expected '(' after the operator")?;
        let first = self.elems.len();
        loop {
            let operand = self.parse_typed_const()?;
            self.elems.push(operand);
            if !self.eat(Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen, "expected ')' after the operands")?;
        let id = self.module.consts.expr(op, ty, None, &self.elems[first..]);
        self.elems.truncate(first);
        ir(at, id)
    }

    /// `blockaddress(@function, %block)`, after the keyword.
    fn block_address(&mut self, at: u32) -> PResult<ConstId> {
        self.expect(Tok::LParen, "expected '(' after blockaddress")?;
        let function = self.tok;
        let sym = self.global_ref(function)?;
        self.bump();
        self.expect(Tok::Comma, "expected ',' after the function")?;
        let label = self.tok;
        if !matches!(label.kind, Tok::Local | Tok::LocalQuoted | Tok::LocalId) {
            return self.syntax("expected a block name");
        }
        self.bump();
        self.expect(Tok::RParen, "expected ')' after the block")?;
        if let Some(ordinal) = self.block_ordinal(function, label)? {
            return ir(at, self.module.consts.block_addr(sym, BlockId::new(ordinal)));
        }
        // A block with no label of its own: only a body parser knows its
        // number.
        self.note("blockaddress:unlabelled-block");
        ir(at, self.module.consts.null(&self.module.types, TypeId::PTR))
    }

    /// The ordinal, in layout order, of the block of the function named
    /// by `function` that the label token `label` names (§5.5); `None`
    /// if no label of the body is that one.
    fn block_ordinal(&mut self, function: Token, label: Token) -> PResult<Option<u32>> {
        if self.defined.is_none() {
            let mut defined = HashMap::new();
            for (at, item) in self.index.items.iter().enumerate() {
                if item.kind == ItemKind::Define
                    && let Some(key) = self.label_key(item.name)
                {
                    defined.entry(key).or_insert(at);
                }
            }
            self.defined = Some(defined);
        }
        let found = self
            .label_key(function)
            .and_then(|key| self.defined.as_ref().expect("filled above").get(&key).copied());
        let Some(found) = found else {
            return fail(
                LlErrorKind::Undefined,
                function.start,
                "blockaddress of a function this module does not define",
            );
        };
        let Some(key) = self.label_key(label) else {
            return Ok(None);
        };
        let (src, body) = (self.src, self.index.items[found].body);
        let labels = self.labels.entry(found).or_insert_with(|| block_labels(src, body));
        let at = labels.binary_search_by(|(name, _)| name.cmp(&key));
        Ok(at.ok().map(|at| labels[at].1))
    }

    /// The key of a `@name`, `%name`, `@42` or `%42` token: a number and
    /// a name that is its digits are not the same thing.
    fn label_key(&self, token: Token) -> Option<LabelKey> {
        if let Some(name) = self.name(token) {
            return Some((false, name.into_owned()));
        }
        let number = parse_u32(&self.tok_text(token)[1..])?;
        Some((true, number.to_string().into_bytes()))
    }
}

/// A block label: whether it is a number, and its name or digits.
pub(crate) type LabelKey = (bool, Vec<u8>);

/// Every label of the body `body`, with the ordinal of the block it
/// starts, sorted by label; of two blocks with one label, the first. A
/// block has exactly one terminator, so a label's ordinal is the number
/// of terminators before it; a terminator keyword is nothing else
/// anywhere in a body, and a label outside parentheses is a block's
/// (inside, `line:` is a field of a debug location).
fn block_labels(src: &[u8], body: crate::Span) -> Vec<(LabelKey, u32)> {
    let mut lexer = Lexer::new(src);
    lexer.set_pos(body.start as usize);
    let mut labels: Vec<(LabelKey, u32)> = Vec::new();
    let mut ordinal = 0u32;
    let mut parens = 0u32;
    loop {
        let token = lexer.next_token();
        if token.start >= body.end || token.kind == Tok::Eof {
            // Stable: equal labels stay in layout order.
            labels.sort_by(|a, b| a.0.cmp(&b.0));
            labels.dedup_by(|later, first| later.0 == first.0);
            return labels;
        }
        let text = lexer.text(token);
        match token.kind {
            Tok::LParen => parens += 1,
            Tok::RParen => parens = parens.saturating_sub(1),
            _ if parens > 0 => {}
            Tok::Label => labels.push(((false, text[..text.len() - 1].to_vec()), ordinal)),
            Tok::LabelId => {
                if let Some(number) = parse_u32(&text[..text.len() - 1]) {
                    labels.push(((true, number.to_string().into_bytes()), ordinal));
                }
            }
            Tok::LabelQuoted => {
                let mut name = Vec::new();
                unescape(&text[1..text.len() - 2], &mut name);
                labels.push(((false, name), ordinal));
            }
            Tok::Word if is_terminator(text) => ordinal += 1,
            _ => {}
        }
    }
}

fn is_terminator(word: &[u8]) -> bool {
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
