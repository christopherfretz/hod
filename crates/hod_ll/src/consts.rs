//! Constants (`spec/ll.md` §5.3 to §5.5): the whole constant syntax of
//! LLVM 19 to 22, built through `hod_ir`'s folding functions so that an
//! address always arrives in its normal form.

use hod_ir::Layering;
use std::collections::HashMap;

use hod_ir::{BlockId, ConstId, ConstOp, SymId, TypeId, TypeKind};

use crate::body::is_terminator;
use crate::error::{LlErrorKind, PResult, fail};
use crate::float::{Narrow, decimal, fp80, fp128, hex_u64, hex_value_u64, narrow};
use crate::index::{ItemKind, parse_u32};
use crate::kw::Kw;
use crate::lexer::{Lexer, Tok, Token, hex_value, unescape};
use crate::limits::MAX_INT_LITERAL_DIGITS;
use crate::parser::{Parser, ir};

impl<L: Layering> Parser<'_, '_, L> {
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
        let word = self.tok.kw;
        let (consts, types) = (&mut self.module.consts, &self.module.types);
        let simple = match word {
            Kw::True | Kw::False if ty == TypeId::I1 => {
                consts.int(types, ty, u64::from(word == Kw::True))
            }
            Kw::True | Kw::False => {
                return fail(LlErrorKind::Type, at, "a boolean constant must have type i1");
            }
            Kw::Null => consts.null(types, ty),
            Kw::Zeroinitializer => consts.zero(types, ty),
            Kw::Undef => consts.undef(types, ty),
            Kw::Poison => consts.poison(types, ty),
            Kw::None if ty == TypeId::TOKEN => consts.none_token(),
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
        let word = self.tok.kw;
        let cast = |op| Some((op, false));
        let binary = |op| Some((op, true));
        let operator = match word {
            Kw::Trunc => cast(ConstOp::Trunc),
            Kw::Ptrtoint => cast(ConstOp::PtrToInt),
            Kw::Ptrtoaddr => cast(ConstOp::PtrToAddr),
            Kw::Inttoptr => cast(ConstOp::IntToPtr),
            Kw::Bitcast => cast(ConstOp::BitCast),
            Kw::Addrspacecast => cast(ConstOp::AddrSpaceCast),
            Kw::Add => binary(ConstOp::Add),
            Kw::Sub => binary(ConstOp::Sub),
            Kw::Mul => binary(ConstOp::Mul),
            Kw::Xor => binary(ConstOp::Xor),
            _ => None,
        };
        self.enter()?;
        self.bump();
        let id = match (word, operator) {
            (_, Some((op, false))) => {
                self.expect(Tok::LParen, "expected '(' after a cast")?;
                let value = self.parse_typed_const()?;
                self.expect_word(Kw::To, "expected 'to' in a cast")?;
                let to = self.parse_type()?;
                self.expect(Tok::RParen, "expected ')' at the end of a cast")?;
                ir(at, self.module.consts.fold_cast(&self.module.types, op, value, to))?
            }
            (_, Some((op, true))) => {
                while self.at_word(Kw::Nuw) || self.at_word(Kw::Nsw) {
                    self.bump();
                }
                self.expect(Tok::LParen, "expected '(' after a binary operator")?;
                let lhs = self.parse_typed_const()?;
                self.expect(Tok::Comma, "expected ',' between operands")?;
                let rhs = self.parse_typed_const()?;
                self.expect(Tok::RParen, "expected ')' after the operands")?;
                ir(at, self.module.consts.fold_binary(&self.module.types, op, lhs, rhs))?
            }
            (Kw::C, _) => {
                let text = self.expect(Tok::Str, "expected a string after 'c'")?;
                self.buf.clear();
                let src = self.src;
                unescape(&src[text.start as usize + 1..text.end as usize - 1], &mut self.buf);
                ir(at, self.module.consts.bytes(&mut self.module.types, &self.buf))?
            }
            (Kw::Getelementptr, _) => self.gep(at)?,
            (Kw::Extractelement, _) => self.vector_op(ConstOp::ExtractElement, ty, at)?,
            (Kw::Insertelement, _) => self.vector_op(ConstOp::InsertElement, ty, at)?,
            (Kw::Shufflevector, _) => self.vector_op(ConstOp::ShuffleVector, ty, at)?,
            (Kw::Blockaddress, _) => self.block_address(at)?,
            (Kw::DsoLocalEquivalent, _) => {
                let sym = self.global_ref(self.tok)?;
                self.bump();
                ir(at, self.module.consts.dso_local_equivalent(sym))?
            }
            (Kw::NoCfi, _) => {
                let sym = self.global_ref(self.tok)?;
                self.bump();
                ir(at, self.module.consts.no_cfi(sym))?
            }
            (Kw::Splat, _) => {
                self.expect(Tok::LParen, "expected '(' after splat")?;
                let elem = self.parse_typed_const()?;
                self.expect(Tok::RParen, "expected ')' after the splat value")?;
                ir(at, self.module.consts.splat(&self.module.types, ty, elem))?
            }
            (Kw::Ptrauth, _) => {
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
            if self.at_word(Kw::Inrange) {
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
        let module = &mut *self.module;
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
        if let Some(ordinal) = self.block_ordinal(function, sym, label)? {
            return ir(at, self.module.consts.block_addr(sym, BlockId::new(ordinal)));
        }
        // A block with no label of its own in a body the scan of §5.5
        // could not number.
        self.note("blockaddress:unlabelled-block");
        ir(at, self.module.consts.null(&self.module.types, TypeId::PTR))
    }

    /// The ordinal, in layout order, of the block of the function named
    /// by `function` that the label token `label` names (§5.5); `None`
    /// if no label of the body is that one.
    fn block_ordinal(&mut self, function: Token, sym: SymId, label: Token) -> PResult<Option<u32>> {
        if let Some(text) = self.text {
            // In a body (`spec/ll.md` §11.3): the definitions are the
            // module text's, found by symbol.
            if self.by_sym.is_empty() && !text.functions().is_empty() {
                let functions = text.functions().iter().enumerate();
                self.by_sym = functions.map(|(at, f)| (f.sym.raw(), at as u32)).collect();
                self.by_sym.sort_unstable();
            }
            let at = self.by_sym.binary_search_by_key(&sym.raw(), |&(raw, _)| raw);
            let Ok(at) = at else {
                return fail(
                    LlErrorKind::Undefined,
                    function.start,
                    "blockaddress of a function this module does not define",
                );
            };
            let found = self.by_sym[at].1 as usize;
            let function = &text.functions()[found];
            return self.find_label(found, function.params, function.body, label);
        }
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
        let item = &self.index.items[found];
        self.find_label(found, item.params, item.body, label)
    }

    /// The ordinal of the block that `label` names in the function
    /// `found` with parameters `params` and body `body`; `None` for a
    /// number the scan could not settle (§5.5). A name or a number the
    /// function has no block for is `Undefined`, as in `llvm-as`.
    fn find_label(
        &mut self,
        found: usize,
        params: crate::Span,
        body: crate::Span,
        label: Token,
    ) -> PResult<Option<u32>> {
        let Some(key) = self.label_key(label) else {
            return Ok(None);
        };
        let src = self.src;
        let labels = self.labels.entry(found).or_insert_with(|| block_labels(src, params, body));
        if let Ok(at) = labels.labels.binary_search_by(|(name, _)| name.cmp(&key)) {
            return Ok(Some(labels.labels[at].1));
        }
        if key.0 && !labels.numbered {
            return Ok(None);
        }
        fail(LlErrorKind::Undefined, label.start, "referenced value is not a basic block")
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

/// The blocks of one body by label (§5.5).
#[derive(Debug, Default)]
pub(crate) struct BlockLabels {
    /// Every label with the ordinal of the block it starts, sorted by
    /// label; of two blocks with one label, the first. With `numbered`,
    /// also the number of each block without a label.
    pub labels: Vec<(LabelKey, u32)>,
    /// Whether the numbers of the blocks without a label are in
    /// `labels`: the scan settled every number of the function.
    pub numbered: bool,
}

/// The labels of the body `body` of a function with parameters `params`.
///
/// A block has exactly one terminator, so a label's ordinal is the number
/// of terminators before it; a terminator keyword is nothing else
/// anywhere in a body, and a label outside parentheses is a block's
/// (inside, `line:` is a field of a debug location).
///
/// A block without a label takes the next number of the function's
/// unnamed values, as in LLVM: the numbers go to unnamed parameters, then
/// to the entry block and every other block without a label, and to each
/// result without a name, in text order; a written number continues the
/// count from itself. The scan reads one statement per line: a line that
/// starts outside brackets with a name and `=` assigns a result, with an
/// instruction word an unassigned instruction (which takes a number
/// unless it has no result: `store`, `fence`, the terminators without
/// one, and a call of a `void` type), with `to`, `unwind`, `catch`,
/// `filter` or `cleanup` continues the line before. Anything else, and a
/// written number below the count, stops the numbering: `numbered` is
/// then `false` and only the written labels are known.
fn block_labels(src: &[u8], params: crate::Span, body: crate::Span) -> BlockLabels {
    let mut lexer = Lexer::new(src);
    let mut labels: Vec<(LabelKey, u32)> = Vec::new();
    let mut numbers = Numbering::default();
    numbers.parameters(&mut lexer, params);
    lexer.set_pos(body.start as usize);
    let mut ordinal = 0u32;
    let mut parens = 0u32;
    let mut brackets = 0u32;
    let mut new_block = true;
    let mut last_end = body.start;
    let mut ahead: Option<Token> = None;
    let mut expect = Expect::Any;
    loop {
        let token = ahead.take().unwrap_or_else(|| lexer.next_token());
        if token.start >= body.end || token.kind == Tok::Eof {
            // Stable: equal labels stay in layout order.
            labels.sort_by(|a, b| a.0.cmp(&b.0));
            labels.dedup_by(|later, first| later.0 == first.0);
            return BlockLabels { labels, numbered: numbers.ok };
        }
        let line_start = parens == 0
            && brackets == 0
            && src[last_end as usize..token.start as usize].contains(&b'\n');
        let line_start = line_start || token.start == body.start || last_end == body.start;
        last_end = token.end;
        let text = lexer.text(token);
        match token.kind {
            Tok::LParen => {
                parens += 1;
                expect = Expect::Any;
            }
            Tok::RParen => parens = parens.saturating_sub(1),
            Tok::LBracket if parens == 0 => brackets += 1,
            Tok::RBracket if parens == 0 => brackets = brackets.saturating_sub(1),
            _ if parens > 0 => {}
            Tok::Label => {
                labels.push(((false, text[..text.len() - 1].to_vec()), ordinal));
                new_block = false;
            }
            Tok::LabelId => {
                if let Some(number) = parse_u32(&text[..text.len() - 1]) {
                    labels.push(((true, number.to_string().into_bytes()), ordinal));
                    numbers.written(number);
                } else {
                    numbers.ok = false;
                }
                new_block = false;
            }
            Tok::LabelQuoted => {
                let mut name = Vec::new();
                unescape(&text[1..text.len() - 2], &mut name);
                labels.push(((false, name), ordinal));
                new_block = false;
            }
            _ => {
                numbers.ok &= expect.step(token.kind, text, line_start && brackets == 0);
                if line_start && brackets == 0 && numbers.ok {
                    let continued = token.kind == Tok::Word
                        && matches!(text, b"to" | b"unwind" | b"catch" | b"filter" | b"cleanup");
                    if !continued {
                        if new_block {
                            if let Some(number) = numbers.take() {
                                labels.push(((true, number.to_string().into_bytes()), ordinal));
                            }
                            new_block = false;
                        }
                        let next = lexer.next_token();
                        ahead = Some(next);
                        numbers.statement(&mut lexer, token, next, &mut ahead);
                    }
                }
                if token.kind == Tok::Word && is_terminator(text) {
                    ordinal += 1;
                    new_block = true;
                }
            }
        }
    }
}

/// What the guard of [`block_labels`] against a second statement on a
/// line expects of the next token outside parentheses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Expect {
    Any,
    /// `=` after a local name at the start of a line.
    Equal,
    /// An instruction's word, after `=` or a tail marker.
    Opcode,
    /// The operation of an `atomicrmw`, after `volatile`.
    RmwOp,
    /// A constant expression's flags, then its `(`.
    Paren,
}

impl Expect {
    /// Reads one token: `false` if it is an instruction word where none
    /// can stand, which only a second statement on the line explains.
    fn step(&mut self, kind: Tok, text: &[u8], line_start: bool) -> bool {
        let word = kind == Tok::Word;
        let before = std::mem::replace(self, Expect::Any);
        match before {
            Expect::Paren if !line_start => {
                let flag = word && is_flag(text);
                if flag {
                    *self = Expect::Paren;
                }
                return flag;
            }
            Expect::RmwOp => {
                if word && text == b"volatile" {
                    *self = Expect::RmwOp;
                }
                return true;
            }
            Expect::Equal if kind == Tok::Equal => {
                *self = Expect::Opcode;
                return true;
            }
            _ => {}
        }
        let opcode_at = line_start || before == Expect::Opcode;
        if word && !opcode_at && (is_terminator(text) || is_instruction(text)) {
            // Inside a statement, only a constant expression's word.
            *self = Expect::Paren;
            return !is_terminator(text);
        }
        if word && text == b"atomicrmw" {
            *self = Expect::RmwOp;
        } else if opcode_at && word && matches!(text, b"tail" | b"musttail" | b"notail") {
            *self = Expect::Opcode;
        } else if line_start && matches!(kind, Tok::Local | Tok::LocalQuoted | Tok::LocalId) {
            *self = Expect::Equal;
        }
        true
    }
}

/// Whether `word` is an instruction's (LLVM's opcode names), so that
/// inside a statement it can only start a constant expression.
fn is_instruction(word: &[u8]) -> bool {
    matches!(
        word,
        b"fneg"
            | b"add"
            | b"fadd"
            | b"sub"
            | b"fsub"
            | b"mul"
            | b"fmul"
            | b"udiv"
            | b"sdiv"
            | b"fdiv"
            | b"urem"
            | b"srem"
            | b"frem"
            | b"shl"
            | b"lshr"
            | b"ashr"
            | b"and"
            | b"or"
            | b"xor"
            | b"extractelement"
            | b"insertelement"
            | b"shufflevector"
            | b"extractvalue"
            | b"insertvalue"
            | b"alloca"
            | b"load"
            | b"store"
            | b"fence"
            | b"cmpxchg"
            | b"atomicrmw"
            | b"getelementptr"
            | b"trunc"
            | b"zext"
            | b"sext"
            | b"fptrunc"
            | b"fpext"
            | b"fptoui"
            | b"fptosi"
            | b"uitofp"
            | b"sitofp"
            | b"ptrtoint"
            | b"ptrtoaddr"
            | b"inttoptr"
            | b"bitcast"
            | b"addrspacecast"
            | b"icmp"
            | b"fcmp"
            | b"phi"
            | b"select"
            | b"freeze"
            | b"call"
            | b"va_arg"
            | b"landingpad"
            | b"catchpad"
            | b"cleanuppad"
    )
}

/// Whether `word` is a flag that may stand between a constant
/// expression's word and its `(`.
fn is_flag(word: &[u8]) -> bool {
    matches!(word, b"nuw" | b"nsw" | b"exact" | b"inbounds" | b"nusw" | b"inrange" | b"disjoint")
}

/// The count of a function's unnamed values while [`block_labels`]
/// scans it.
struct Numbering {
    next: u64,
    ok: bool,
}

impl Default for Numbering {
    fn default() -> Self {
        Numbering { next: 0, ok: true }
    }
}

impl Numbering {
    /// The next number, taken.
    fn take(&mut self) -> Option<u32> {
        let number = u32::try_from(self.next).ok();
        self.next += 1;
        self.ok &= number.is_some();
        number
    }

    /// A number written in the text: the count continues after it.
    fn written(&mut self, number: u32) {
        if u64::from(number) < self.next {
            self.ok = false;
        }
        self.next = u64::from(number) + 1;
    }

    /// The parameters: each without a name takes a number. A parameter
    /// is named when it ends in a local name that is not its first token
    /// (`%T` alone is a type).
    fn parameters(&mut self, lexer: &mut Lexer<'_>, params: crate::Span) {
        lexer.set_pos(params.start as usize);
        let (mut depth, mut tokens, mut last) = (0u32, 0u32, None::<Token>);
        loop {
            let token = lexer.next_token();
            let end = token.start >= params.end || token.kind == Tok::Eof;
            if end || (depth == 0 && token.kind == Tok::Comma) {
                if tokens > 0 {
                    match last {
                        Some(name) if tokens > 1 && name.kind == Tok::LocalId => {
                            match parse_u32(&lexer.text(name)[1..]) {
                                Some(number) => self.written(number),
                                None => self.ok = false,
                            }
                        }
                        Some(name)
                            if tokens > 1 && matches!(name.kind, Tok::Local | Tok::LocalQuoted) => {
                        }
                        Some(token) if token.kind == Tok::DotDotDot => {}
                        _ => {
                            self.take();
                        }
                    }
                }
                if end {
                    return;
                }
                (tokens, last) = (0, None);
                continue;
            }
            match token.kind {
                Tok::LParen | Tok::LBracket | Tok::LBrace | Tok::Less => depth += 1,
                Tok::RParen | Tok::RBracket | Tok::RBrace | Tok::Greater => {
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
            tokens += 1;
            last = Some(token);
        }
    }

    /// The statement that starts with `first`, then `second`: an
    /// assignment, or an unassigned instruction.
    fn statement(
        &mut self,
        lexer: &mut Lexer<'_>,
        first: Token,
        second: Token,
        ahead: &mut Option<Token>,
    ) {
        match first.kind {
            Tok::LocalId if second.kind == Tok::Equal => match parse_u32(&lexer.text(first)[1..]) {
                Some(number) => self.written(number),
                None => self.ok = false,
            },
            Tok::Local | Tok::LocalQuoted if second.kind == Tok::Equal => {}
            // A debug record (`#dbg_value(…)`) is no instruction.
            Tok::HashWord => {}
            Tok::Word => {
                let mut word = lexer.text(first);
                let mut next = second;
                if matches!(word, b"tail" | b"musttail" | b"notail") && next.kind == Tok::Word {
                    word = lexer.text(next);
                    next = lexer.next_token();
                    *ahead = Some(next);
                }
                let result = match word {
                    b"store" | b"fence" | b"br" | b"ret" | b"switch" | b"indirectbr"
                    | b"resume" | b"unreachable" | b"cleanupret" | b"catchret" => false,
                    b"call" | b"invoke" | b"callbr" => {
                        let Some(void) = return_type_is_void(lexer, next, ahead) else {
                            self.ok = false;
                            return;
                        };
                        !void
                    }
                    _ => true,
                };
                if result {
                    self.take();
                }
            }
            _ => self.ok = false,
        }
    }
}

/// Whether the return type of the call whose words after the opcode
/// start with `first` is `void`: the first type after the fast-math
/// flags, calling convention and return attributes. `None` if the line
/// ends first.
fn return_type_is_void(
    lexer: &mut Lexer<'_>,
    first: Token,
    ahead: &mut Option<Token>,
) -> Option<bool> {
    let mut token = first;
    let mut parens = 0u32;
    loop {
        let text = lexer.text(token);
        match token.kind {
            Tok::Eof => return None,
            Tok::LParen => parens += 1,
            Tok::RParen => parens = parens.saturating_sub(1),
            _ if parens > 0 => {}
            Tok::Word if text == b"void" => return Some(true),
            Tok::Word
                if matches!(
                    text,
                    b"ptr"
                        | b"half"
                        | b"bfloat"
                        | b"float"
                        | b"double"
                        | b"x86_fp80"
                        | b"fp128"
                        | b"ppc_fp128"
                        | b"x86_amx"
                        | b"x86_mmx"
                        | b"token"
                        | b"label"
                        | b"metadata"
                        | b"target"
                ) =>
            {
                return Some(false);
            }
            Tok::IntType
            | Tok::LBrace
            | Tok::Less
            | Tok::LBracket
            | Tok::Local
            | Tok::LocalQuoted
            | Tok::LocalId => return Some(false),
            Tok::Word | Tok::Int | Tok::Str | Tok::Equal => {}
            _ => return None,
        }
        token = lexer.next_token();
        *ahead = Some(token);
    }
}
