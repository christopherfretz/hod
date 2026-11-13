//! Types (`spec/ll.md` §5.1, §5.2): every type of `spec/ir.md` §3, and
//! named types resolved on first use, in whatever order they are written.

use hod_ir::Layering;
use hod_ir::TypeId;

use crate::error::{LlErrorKind, PResult, fail};
use crate::index::parse_u32;
use crate::kw::Kw;
use crate::lexer::{Lexer, Tok, Token};
use crate::parser::{Parser, TypeSlot, ir};

impl<L: Layering> Parser<'_, '_, L> {
    /// Registers every `%name = type …` and resolves each.
    pub(crate) fn define_types(&mut self) -> PResult<()> {
        self.type_slots = vec![TypeSlot::Pending; self.index.types.len()];
        for at in 0..self.index.types.len() {
            let token = self.index.types[at].name;
            if let Some(name) = self.name(token) {
                if self.type_names.insert(&name, at as u32).is_err() {
                    return fail(LlErrorKind::Redefined, token.start, "redefinition of type");
                }
            } else {
                let number = self.type_number(token)?;
                self.type_numbers.push((number, at as u32));
            }
        }
        // Numbered types come in any order: sorted once, and two of one
        // number are then neighbours, the later definition second.
        self.type_numbers.sort_unstable();
        if let Some(pair) = self.type_numbers.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            let token = self.index.types[pair[1].1 as usize].name;
            return fail(LlErrorKind::Redefined, token.start, "redefinition of type");
        }
        for at in self.type_order()? {
            let token = self.index.types[at as usize].name;
            self.resolve_type(at as usize, token.start)?;
        }
        Ok(())
    }

    /// The definitions in an order in which each comes after the types it
    /// holds, wherever they stand in the file: a walk over the names each
    /// definition mentions, with its own stack, so that a chain of types
    /// each holding the next may be as long as the file. A type that
    /// holds itself is refused here, at the name that closes the circle.
    fn type_order(&mut self) -> PResult<Vec<u32>> {
        const NEW: u8 = 0;
        const OPEN: u8 = 1;
        const DONE: u8 = 2;
        let count = self.index.types.len();
        let mut state = vec![NEW; count];
        let mut order = Vec::with_capacity(count);
        // A definition being walked, and the offset to go on from.
        let mut stack: Vec<(u32, u32)> = Vec::new();
        let mut lexer = Lexer::new(self.src);
        for root in 0..count {
            if state[root] != NEW {
                continue;
            }
            state[root] = OPEN;
            stack.push((root as u32, self.index.types[root].start));
            while let Some(&(slot, from)) = stack.last() {
                let end = self.index.types[slot as usize].end;
                lexer.set_pos(from as usize);
                let mut held = None;
                loop {
                    let token = lexer.next_token();
                    if token.start >= end || matches!(token.kind, Tok::Eof | Tok::Error) {
                        break;
                    }
                    if !matches!(token.kind, Tok::Local | Tok::LocalQuoted | Tok::LocalId) {
                        continue;
                    }
                    // `%T*`, the pointer of old: it does not hold `%T`.
                    let after = lexer.pos();
                    let pointer = lexer.next_token().kind == Tok::Star;
                    lexer.set_pos(after);
                    if pointer {
                        continue;
                    }
                    let Some(to) = self.type_slot(token) else {
                        continue;
                    };
                    match state[to as usize] {
                        NEW => {
                            held = Some((to, token.end));
                            break;
                        }
                        OPEN => {
                            let message = "a type cannot contain itself";
                            return fail(LlErrorKind::Type, token.start, message);
                        }
                        _ => {}
                    }
                }
                if let Some((to, resume)) = held {
                    stack.last_mut().expect("the definition being walked").1 = resume;
                    state[to as usize] = OPEN;
                    stack.push((to, self.index.types[to as usize].start));
                } else {
                    state[slot as usize] = DONE;
                    order.push(slot);
                    stack.pop();
                }
            }
        }
        Ok(order)
    }

    /// The definition a `%name` or `%42` token names, if there is one.
    fn type_slot(&self, token: Token) -> Option<u32> {
        if let Some(name) = self.name(token) {
            return self.type_names.get(&name);
        }
        let number = parse_u32(&self.tok_text(token)[1..])?;
        let found = self.type_numbers.binary_search_by_key(&number, |&(n, _)| n);
        found.ok().map(|place| self.type_numbers[place].1)
    }

    fn type_number(&self, token: Token) -> PResult<u32> {
        match parse_u32(&self.tok_text(token)[1..]) {
            Some(number) => Ok(number),
            None => fail(LlErrorKind::Syntax, token.start, "type number is too large"),
        }
    }

    /// The type that definition `at` gives, parsing the definition if it
    /// has not been yet. `from` is where the reference stands.
    fn resolve_type(&mut self, at: usize, from: u32) -> PResult<TypeId> {
        match self.type_slots[at] {
            TypeSlot::Done(ty) => return Ok(ty),
            TypeSlot::Busy => {
                return fail(LlErrorKind::Type, from, "a type cannot contain itself");
            }
            TypeSlot::Pending => {}
        }
        self.type_slots[at] = TypeSlot::Busy;
        let saved = (self.lx.pos(), self.tok);
        let ent = self.index.types[at];
        self.enter()?;
        self.goto(ent.start);
        let ty = if self.at_word(Kw::Opaque) {
            let opaque = self.module.types.opaque_struct();
            self.bump();
            ir(ent.name.start, opaque)?
        } else {
            self.parse_type()?
        };
        if self.tok.start < ent.end {
            return self.syntax("expected the end of the type definition");
        }
        self.leave();
        self.lx.set_pos(saved.0);
        self.tok = saved.1;
        self.type_slots[at] = TypeSlot::Done(ty);
        Ok(ty)
    }

    /// The type a `%name` token refers to. `pointer` says that a `*`
    /// follows: the pointer of old needs the name to be a type's, not
    /// the type, so a type may hold a pointer to itself.
    fn named_type(&mut self, token: Token, pointer: bool) -> PResult<TypeId> {
        if token.kind == Tok::LocalId {
            self.type_number(token)?;
        }
        if let Some(text) = self.text {
            // A function body: every named type is resolved.
            let found = match self.name(token) {
                Some(name) => text.named_type(&name),
                None => text.numbered_type(self.type_number(token)?),
            };
            return match found {
                Some(_) if pointer => Ok(TypeId::PTR),
                Some(ty) => Ok(ty),
                None => self.undefined_type(token),
            };
        }
        match self.type_slot(token) {
            Some(_) if pointer => Ok(TypeId::PTR),
            Some(slot) => self.resolve_type(slot as usize, token.start),
            None => self.undefined_type(token),
        }
    }

    fn undefined_type(&self, token: Token) -> PResult<TypeId> {
        fail(
            LlErrorKind::Undefined,
            token.start,
            format!(
                "use of undefined type named '{}'",
                String::from_utf8_lossy(&self.tok_text(token)[1..])
            ),
        )
    }

    /// Parses one type.
    pub(crate) fn parse_type(&mut self) -> PResult<TypeId> {
        let at = self.tok.start;
        let mut ty = match self.tok.kind {
            Tok::IntType => {
                let ty = match self.text() {
                    b"i8" => TypeId::I8,
                    b"i32" => TypeId::I32,
                    b"i64" => TypeId::I64,
                    b"i16" => TypeId::I16,
                    b"i1" => TypeId::I1,
                    text => {
                        let bits = parse_u32(&text[1..]).unwrap_or(u32::MAX);
                        ir(at, self.module.types.int(bits))?
                    }
                };
                self.bump();
                ty
            }
            Tok::Word => self.word_type()?,
            Tok::Local | Tok::LocalQuoted | Tok::LocalId => {
                let name = self.tok;
                self.bump();
                let pointer = self.tok.kind == Tok::Star;
                let ty = self.named_type(name, pointer)?;
                if pointer {
                    self.bump();
                }
                ty
            }
            Tok::LBrace => self.struct_type(false)?,
            Tok::LBracket => self.array_type()?,
            Tok::Less => self.angle_type()?,
            _ => return self.syntax("expected a type"),
        };
        loop {
            match self.tok.kind {
                Tok::LParen => ty = self.function_type(ty, at)?,
                // `T*` and `T addrspace(N)*`: the pointer types of LLVM
                // before 15, which its reader still takes for `ptr`.
                Tok::Star => {
                    self.pointee(ty)?;
                    self.bump();
                    ty = TypeId::PTR;
                }
                Tok::Word if self.at_word(Kw::Addrspace) => {
                    self.pointee(ty)?;
                    self.bump();
                    self.expect(Tok::LParen, "expected '(' in an address space")?;
                    let space = self.expect_u32("expected an address space number")?;
                    self.expect(Tok::RParen, "expected ')' in an address space")?;
                    self.expect(Tok::Star, "expected '*' after an address space")?;
                    ty = ir(at, self.module.types.ptr(space))?;
                }
                _ => return Ok(ty),
            }
        }
    }

    /// Checks that `ty` is something a pointer was once written to.
    fn pointee(&mut self, ty: TypeId) -> PResult<()> {
        if matches!(ty, TypeId::VOID | TypeId::LABEL | TypeId::METADATA | TypeId::TOKEN) {
            return self.syntax("a pointer to this type is invalid");
        }
        Ok(())
    }

    fn word_type(&mut self) -> PResult<TypeId> {
        let at = self.tok.start;
        let ty = match self.text() {
            b"ptr" => {
                self.bump();
                if !self.eat_word(Kw::Addrspace) {
                    return Ok(TypeId::PTR);
                }
                self.expect(Tok::LParen, "expected '(' in an address space")?;
                let space = self.expect_u32("expected an address space number")?;
                self.expect(Tok::RParen, "expected ')' in an address space")?;
                return ir(at, self.module.types.ptr(space));
            }
            b"void" => TypeId::VOID,
            b"half" => TypeId::HALF,
            b"bfloat" => TypeId::BFLOAT,
            b"float" => TypeId::FLOAT,
            b"double" => TypeId::DOUBLE,
            b"x86_fp80" => TypeId::X86_FP80,
            b"fp128" => TypeId::FP128,
            b"label" => TypeId::LABEL,
            b"metadata" => TypeId::METADATA,
            b"token" => TypeId::TOKEN,
            b"ppc_fp128" => {
                // Not a type of `hod_ir`: noted, and stood in for by the
                // other 128-bit format so that the parse goes on.
                self.note("type:ppc_fp128");
                TypeId::FP128
            }
            b"x86_mmx" => {
                // A type of LLVM 19 and before; LLVM 20 reads it out of
                // old bitcode as the vector it now uses in its place.
                self.note("type:x86_mmx");
                ir(at, self.module.types.vector(TypeId::I64, 1))?
            }
            b"x86_amx" => {
                self.note("type:x86_amx");
                ir(at, self.module.types.target_ext("x86_amx"))?
            }
            b"target" => return self.target_type(),
            _ => return self.syntax("expected a type"),
        };
        self.bump();
        Ok(ty)
    }

    /// `target("name", types…, integers…)`.
    fn target_type(&mut self) -> PResult<TypeId> {
        let at = self.tok.start;
        self.bump();
        self.expect(Tok::LParen, "expected '(' in a target extension type")?;
        let name = self.expect(Tok::Str, "expected the target extension type's name")?;
        let name = self.string(name);
        while self.eat(Tok::Comma) {
            if self.tok.kind == Tok::Int {
                self.bump();
            } else {
                self.parse_type()?;
            }
        }
        self.expect(Tok::RParen, "expected ')' in a target extension type")?;
        ir(at, self.module.types.target_ext(&String::from_utf8_lossy(&name)))
    }

    /// `{ … }`, at the brace.
    fn struct_type(&mut self, packed: bool) -> PResult<TypeId> {
        let at = self.tok.start;
        self.enter()?;
        self.bump();
        let base = self.tys.len();
        if self.tok.kind != Tok::RBrace {
            loop {
                let field = self.parse_type()?;
                self.tys.push(field);
                if !self.eat(Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(Tok::RBrace, "expected '}' at the end of a struct type")?;
        let ty = self.module.types.struct_type(&self.tys[base..], packed);
        self.tys.truncate(base);
        self.leave();
        ir(at, ty)
    }

    /// `[N x T]`.
    fn array_type(&mut self) -> PResult<TypeId> {
        let at = self.tok.start;
        self.enter()?;
        self.bump();
        let len = self.expect_u64("expected the number of elements")?;
        self.expect_word(Kw::X, "expected 'x' after the number of elements")?;
        let elem = self.parse_type()?;
        self.expect(Tok::RBracket, "expected ']' at the end of an array type")?;
        self.leave();
        ir(at, self.module.types.array(elem, len))
    }

    /// `<N x T>`, `<vscale x N x T>` or `<{ … }>`.
    fn angle_type(&mut self) -> PResult<TypeId> {
        let at = self.tok.start;
        self.enter()?;
        self.bump();
        if self.tok.kind == Tok::LBrace {
            let ty = self.struct_type(true)?;
            self.expect(Tok::Greater, "expected '>' at the end of a packed struct type")?;
            self.leave();
            return Ok(ty);
        }
        let scalable = self.eat_word(Kw::Vscale);
        if scalable {
            self.expect_word(Kw::X, "expected 'x' after vscale")?;
        }
        let len = self.expect_u32("expected the number of elements")?;
        self.expect_word(Kw::X, "expected 'x' after the number of elements")?;
        let elem = self.parse_type()?;
        self.expect(Tok::Greater, "expected '>' at the end of a vector type")?;
        self.leave();
        let types = &mut self.module.types;
        ir(at, if scalable { types.scalable_vector(elem, len) } else { types.vector(elem, len) })
    }

    /// `ret (params…)`, at the parenthesis.
    fn function_type(&mut self, ret: TypeId, at: u32) -> PResult<TypeId> {
        self.enter()?;
        self.bump();
        let base = self.tys.len();
        let mut variadic = false;
        if self.tok.kind != Tok::RParen {
            loop {
                if self.eat(Tok::DotDotDot) {
                    variadic = true;
                    break;
                }
                let param = self.parse_type()?;
                self.tys.push(param);
                if !self.eat(Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(Tok::RParen, "expected ')' at the end of a function type")?;
        let ty = self.module.types.function(ret, &self.tys[base..], variadic);
        self.tys.truncate(base);
        self.leave();
        ir(at, ty)
    }
}
