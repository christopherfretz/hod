//! Instructions (`spec/ll.md` §11.4 to §11.6): the grammar of each, the
//! checks LLVM's parser makes on it, and its form in `hod_ir`. Calls are
//! in `call.rs`.

use hod_ir::{
    AtomicOrdering, Clause, Const, ConstId, FloatPredicate, IntPredicate, MemFlags, Opcode, RmwOp,
    SHUFFLE_UNDEF, SyncScope, TailKind, TypeId, TypeKind, Value, aggregate_field,
};

use crate::body::Fx;
use crate::error::{LlErrorKind, PResult, fail};
use crate::lexer::Tok;
use crate::parser::ir;

/// What a statement leaves behind: the type and the value of its result
/// (`void` and no value for most), and whether it ended the block.
pub(crate) type Done = (TypeId, Value, bool);

/// The length of a vector type and whether it is scalable; `None` for
/// any other type.
type Shape = Option<(u32, bool)>;

const FAST_MATH: &[&[u8]] =
    &[b"nnan", b"ninf", b"nsz", b"arcp", b"contract", b"afn", b"reassoc", b"fast"];

/// `value` as a signed integer, sign-extended from `bits` bits.
fn sext64(value: u64, bits: u32) -> i64 {
    if bits >= 64 {
        value as i64
    } else {
        let shift = 64 - bits;
        ((value << shift) as i64) >> shift
    }
}

fn mistyped<T>(at: u32, message: &str) -> PResult<T> {
    fail(LlErrorKind::Type, at, message)
}

impl Fx<'_, '_, '_> {
    // ---- operands ----

    /// The element type and the vector shape of `ty`; a type that is not
    /// a vector is its own element.
    pub(crate) fn shape(&self, ty: TypeId) -> (TypeId, Shape) {
        match self.p.module.types.kind(ty) {
            TypeKind::Vector { elem, len } => (elem, Some((len, false))),
            TypeKind::ScalableVector { elem, min_len } => (elem, Some((min_len, true))),
            _ => (ty, None),
        }
    }

    /// The type with the shape `shape` and elements of type `elem`.
    fn shaped(&mut self, at: u32, elem: TypeId, shape: Shape) -> PResult<TypeId> {
        let types = &mut self.p.module.types;
        match shape {
            None => Ok(elem),
            Some((len, false)) => ir(at, types.vector(elem, len)),
            Some((len, true)) => ir(at, types.scalable_vector(elem, len)),
        }
    }

    /// A value of type `ty`: a local name, a global's address or a
    /// constant.
    pub(crate) fn value(&mut self, ty: TypeId) -> PResult<Value> {
        let token = self.p.tok;
        match token.kind {
            Tok::Local | Tok::LocalQuoted | Tok::LocalId => {
                self.p.bump();
                self.local_value(token, ty)
            }
            Tok::Global | Tok::GlobalQuoted | Tok::GlobalId if ty == TypeId::PTR => {
                let sym = self.p.global_ref(token)?;
                self.p.bump();
                ir(token.start, Value::sym(sym))
            }
            Tok::Word => {
                let id = self.p.parse_const(ty)?;
                // An expression that is just a symbol's address is the
                // symbol: a call of it is a direct call.
                if ty == TypeId::PTR
                    && let Const::Addr { sym, addend: 0 } = self.p.module.consts.get(id)
                {
                    return ir(token.start, Value::sym(sym));
                }
                ir(token.start, Value::constant(id))
            }
            _ => {
                let id = self.p.parse_const(ty)?;
                ir(token.start, Value::constant(id))
            }
        }
    }

    /// `type value`.
    pub(crate) fn typed(&mut self) -> PResult<(TypeId, Value)> {
        let ty = self.p.parse_type()?;
        Ok((ty, self.value(ty)?))
    }

    /// `type value` where the type must be a pointer; `what` names the
    /// instruction.
    fn pointer(&mut self, what: &str) -> PResult<Value> {
        let at = self.p.tok.start;
        let (ty, value) = self.typed()?;
        if !self.p.module.types.is_ptr(ty) {
            return mistyped(at, &format!("{what} operand must be a pointer"));
        }
        Ok(value)
    }

    pub(crate) fn comma(&mut self) -> PResult<()> {
        self.p.expect(Tok::Comma, "expected ',' between operands").map(drop)
    }

    /// Steps over any of the flag words `allowed`.
    fn flags(&mut self, allowed: &[&[u8]]) {
        while self.p.tok.kind == Tok::Word && allowed.contains(&self.p.text()) {
            self.p.bump();
        }
    }

    /// Steps over fast-math flags (`spec/ir.md` §5.4: not kept).
    pub(crate) fn fast_math(&mut self) {
        self.flags(FAST_MATH);
    }

    /// What stands in for the result of an instruction that `hod_ir`
    /// cannot represent, in a body that will not be kept.
    pub(crate) fn stand_in(&mut self, at: u32, ty: TypeId) -> PResult<Value> {
        let module = &mut *self.p.module;
        let id = match ty {
            TypeId::VOID => return Ok(Value::NONE),
            TypeId::TOKEN => module.consts.none_token(),
            _ => module.consts.poison(&module.types, ty),
        };
        ir(at, Value::constant(ir(at, id)?))
    }

    /// Whether a value of type `ty` can be accessed atomically
    /// (`spec/ir.md` §5.4).
    fn atomic_type(&self, ty: TypeId) -> bool {
        let types = &self.p.module.types;
        let (scalar, shape) = self.shape(ty);
        let class = types.is_int(scalar) || types.is_ptr(scalar) || types.is_float(scalar);
        let bits = types.size_in_bits_of(scalar) * shape.map_or(1, |(len, _)| u64::from(len));
        class && bits >= 8 && bits.is_power_of_two()
    }

    /// LLVM's `castIsValid`.
    fn cast_is_valid(&self, op: Opcode, src: TypeId, dst: TypeId) -> bool {
        let types = &self.p.module.types;
        let (s, s_shape) = self.shape(src);
        let (d, d_shape) = self.shape(dst);
        let same = s_shape == d_shape;
        let bits = |ty| types.size_in_bits_of(ty);
        let (int, float, ptr) =
            (|ty| types.is_int(ty), |ty| types.is_float(ty), |ty| types.is_ptr(ty));
        match op {
            Opcode::Trunc => same && int(s) && int(d) && bits(s) > bits(d),
            Opcode::ZExt | Opcode::SExt => same && int(s) && int(d) && bits(s) < bits(d),
            Opcode::FPTrunc => same && float(s) && float(d) && bits(s) > bits(d),
            Opcode::FPExt => same && float(s) && float(d) && bits(s) < bits(d),
            Opcode::FPToUI | Opcode::FPToSI => same && float(s) && int(d),
            Opcode::UIToFP | Opcode::SIToFP => same && int(s) && float(d),
            Opcode::PtrToInt => same && ptr(s) && int(d),
            Opcode::PtrToAddr => {
                let TypeKind::Ptr { addr_space } = types.kind(s) else { return false };
                let address = types.data_layout().pointer(addr_space).index_bits;
                same && int(d) && bits(d) == u64::from(address)
            }
            Opcode::IntToPtr => same && int(s) && ptr(d),
            Opcode::AddrSpaceCast => same && ptr(s) && ptr(d) && s != d,
            Opcode::BitCast => {
                if ptr(s) != ptr(d) {
                    return false;
                }
                if ptr(s) {
                    return s == d
                        && match (s_shape, d_shape) {
                            (Some(a), Some(b)) => a == b,
                            (Some(one), None) | (None, Some(one)) => one == (1, false),
                            (None, None) => true,
                        };
                }
                let value = |ty| int(ty) || float(ty);
                let scalable = |shape: Shape| shape.is_some_and(|(_, scalable)| scalable);
                value(s)
                    && value(d)
                    && bits(src) == bits(dst)
                    && scalable(s_shape) == scalable(d_shape)
            }
            _ => false,
        }
    }

    // ---- the instruction ----

    /// One instruction, at its first word.
    pub(crate) fn instruction(&mut self) -> PResult<Done> {
        let at = self.p.tok.start;
        if self.p.tok.kind != Tok::Word {
            return self.p.syntax("expected instruction opcode");
        }
        let word = self.p.text();
        self.p.bump();
        let (ty, value) = match word {
            b"call" => return self.call(at, Opcode::Call, TailKind::None),
            b"load" => self.load(at)?,
            b"store" => self.store(at)?,
            b"getelementptr" => self.get_element_ptr(at)?,
            b"br" => return self.br(at),
            b"ret" => return self.ret(at),
            b"icmp" => self.icmp(at)?,
            b"alloca" => self.alloca(at)?,
            b"phi" => self.phi(at)?,
            b"add" => self.binary(at, Opcode::Add)?,
            b"sub" => self.binary(at, Opcode::Sub)?,
            b"mul" => self.binary(at, Opcode::Mul)?,
            b"udiv" => self.binary(at, Opcode::UDiv)?,
            b"sdiv" => self.binary(at, Opcode::SDiv)?,
            b"urem" => self.binary(at, Opcode::URem)?,
            b"srem" => self.binary(at, Opcode::SRem)?,
            b"shl" => self.binary(at, Opcode::Shl)?,
            b"lshr" => self.binary(at, Opcode::LShr)?,
            b"ashr" => self.binary(at, Opcode::AShr)?,
            b"and" => self.binary(at, Opcode::And)?,
            b"or" => self.binary(at, Opcode::Or)?,
            b"xor" => self.binary(at, Opcode::Xor)?,
            b"fadd" => self.binary(at, Opcode::FAdd)?,
            b"fsub" => self.binary(at, Opcode::FSub)?,
            b"fmul" => self.binary(at, Opcode::FMul)?,
            b"fdiv" => self.binary(at, Opcode::FDiv)?,
            b"frem" => self.binary(at, Opcode::FRem)?,
            b"fneg" => self.unary(at, true)?,
            b"freeze" => self.unary(at, false)?,
            b"fcmp" => self.fcmp(at)?,
            b"trunc" => self.cast(at, Opcode::Trunc)?,
            b"zext" => self.cast(at, Opcode::ZExt)?,
            b"sext" => self.cast(at, Opcode::SExt)?,
            b"fptrunc" => self.cast(at, Opcode::FPTrunc)?,
            b"fpext" => self.cast(at, Opcode::FPExt)?,
            b"fptoui" => self.cast(at, Opcode::FPToUI)?,
            b"fptosi" => self.cast(at, Opcode::FPToSI)?,
            b"uitofp" => self.cast(at, Opcode::UIToFP)?,
            b"sitofp" => self.cast(at, Opcode::SIToFP)?,
            b"ptrtoint" => self.cast(at, Opcode::PtrToInt)?,
            b"ptrtoaddr" => self.cast(at, Opcode::PtrToAddr)?,
            b"inttoptr" => self.cast(at, Opcode::IntToPtr)?,
            b"bitcast" => self.cast(at, Opcode::BitCast)?,
            b"addrspacecast" => self.cast(at, Opcode::AddrSpaceCast)?,
            b"select" => self.select(at)?,
            b"switch" => return self.switch(at),
            b"unreachable" => {
                ir(at, self.b.unreachable())?;
                return Ok((TypeId::VOID, Value::NONE, true));
            }
            b"invoke" => return self.call(at, Opcode::Invoke, TailKind::None),
            b"callbr" => return self.call(at, Opcode::CallBr, TailKind::None),
            b"tail" => return self.tail_call(at, TailKind::Tail),
            b"musttail" => return self.tail_call(at, TailKind::MustTail),
            b"notail" => return self.tail_call(at, TailKind::NoTail),
            b"extractvalue" => self.extract_value(at)?,
            b"insertvalue" => self.insert_value(at)?,
            b"extractelement" => self.extract_element(at)?,
            b"insertelement" => self.insert_element(at)?,
            b"shufflevector" => self.shuffle_vector(at)?,
            b"landingpad" => self.landing_pad(at)?,
            b"resume" => {
                let (_, value) = self.typed()?;
                ir(at, self.b.resume(value))?;
                return Ok((TypeId::VOID, Value::NONE, true));
            }
            b"indirectbr" => return self.indirect_br(at),
            b"atomicrmw" => self.atomic_rmw(at)?,
            b"cmpxchg" => self.cmpxchg(at)?,
            b"fence" => self.fence(at)?,
            b"va_arg" => self.va_arg(at)?,
            b"catchswitch" => return self.catch_switch(at),
            b"catchpad" => self.pad(at, Opcode::CatchPad)?,
            b"cleanuppad" => self.pad(at, Opcode::CleanupPad)?,
            b"catchret" => return self.catch_ret(at),
            b"cleanupret" => return self.cleanup_ret(at),
            _ => return fail(LlErrorKind::Syntax, at, "expected instruction opcode"),
        };
        Ok((ty, value, false))
    }

    fn tail_call(&mut self, at: u32, tail: TailKind) -> PResult<Done> {
        self.p.expect_word(b"call", "expected 'call' after a tail-call marker")?;
        self.call(at, Opcode::Call, tail)
    }

    // ---- arithmetic, comparisons, casts ----

    fn binary(&mut self, at: u32, op: Opcode) -> PResult<(TypeId, Value)> {
        match op {
            Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Shl => self.flags(&[b"nuw", b"nsw"]),
            Opcode::UDiv | Opcode::SDiv | Opcode::LShr | Opcode::AShr => self.flags(&[b"exact"]),
            Opcode::Or => self.flags(&[b"disjoint"]),
            _ if op.is_float_binary() => self.fast_math(),
            _ => {}
        }
        let ty_at = self.p.tok.start;
        let ty = self.p.parse_type()?;
        let lhs = self.value(ty)?;
        self.comma()?;
        let rhs = self.value(ty)?;
        let scalar = self.shape(ty).0;
        let types = &self.p.module.types;
        let fits = if op.is_int_binary() { types.is_int(scalar) } else { types.is_float(scalar) };
        if !fits {
            return mistyped(ty_at, "invalid operand type for instruction");
        }
        Ok((ty, ir(at, self.b.binary(op, ty, lhs, rhs))?))
    }

    /// `fneg` (`float`) or `freeze`.
    fn unary(&mut self, at: u32, float: bool) -> PResult<(TypeId, Value)> {
        if float {
            self.fast_math();
        }
        let ty_at = self.p.tok.start;
        let (ty, arg) = self.typed()?;
        if !float {
            return Ok((ty, ir(at, self.b.freeze(ty, arg))?));
        }
        if !self.p.module.types.is_float(self.shape(ty).0) {
            return mistyped(ty_at, "invalid operand type for instruction");
        }
        Ok((ty, ir(at, self.b.fneg(ty, arg))?))
    }

    fn icmp(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        self.flags(&[b"samesign"]);
        let pred = match self.p.text() {
            _ if self.p.tok.kind != Tok::Word => None,
            b"eq" => Some(IntPredicate::Eq),
            b"ne" => Some(IntPredicate::Ne),
            b"ugt" => Some(IntPredicate::Ugt),
            b"uge" => Some(IntPredicate::Uge),
            b"ult" => Some(IntPredicate::Ult),
            b"ule" => Some(IntPredicate::Ule),
            b"sgt" => Some(IntPredicate::Sgt),
            b"sge" => Some(IntPredicate::Sge),
            b"slt" => Some(IntPredicate::Slt),
            b"sle" => Some(IntPredicate::Sle),
            _ => None,
        };
        let Some(pred) = pred else {
            return self.p.syntax("expected icmp predicate (e.g. 'eq')");
        };
        self.p.bump();
        let ty_at = self.p.tok.start;
        let ty = self.p.parse_type()?;
        let lhs = self.value(ty)?;
        self.comma()?;
        let rhs = self.value(ty)?;
        let (scalar, shape) = self.shape(ty);
        let types = &self.p.module.types;
        if !types.is_int(scalar) && !types.is_ptr(scalar) {
            return mistyped(ty_at, "icmp requires integer operands");
        }
        let result = self.shaped(ty_at, TypeId::I1, shape)?;
        Ok((result, ir(at, self.b.icmp(pred, result, lhs, rhs))?))
    }

    fn fcmp(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        self.fast_math();
        let found = match self.p.tok.kind {
            Tok::Word => {
                let word = self.p.text();
                FloatPredicate::ALL.iter().copied().find(|pred| pred.name().as_bytes() == word)
            }
            _ => None,
        };
        let Some(pred) = found else {
            return self.p.syntax("expected fcmp predicate (e.g. 'oeq')");
        };
        self.p.bump();
        let ty_at = self.p.tok.start;
        let ty = self.p.parse_type()?;
        let lhs = self.value(ty)?;
        self.comma()?;
        let rhs = self.value(ty)?;
        let (scalar, shape) = self.shape(ty);
        if !self.p.module.types.is_float(scalar) {
            return mistyped(ty_at, "fcmp requires floating point operands");
        }
        let result = self.shaped(ty_at, TypeId::I1, shape)?;
        Ok((result, ir(at, self.b.fcmp(pred, result, lhs, rhs))?))
    }

    fn cast(&mut self, at: u32, op: Opcode) -> PResult<(TypeId, Value)> {
        match op {
            Opcode::Trunc => self.flags(&[b"nuw", b"nsw"]),
            Opcode::ZExt | Opcode::UIToFP => self.flags(&[b"nneg"]),
            _ => {}
        }
        if matches!(op, Opcode::FPTrunc | Opcode::FPExt | Opcode::UIToFP | Opcode::SIToFP) {
            self.fast_math();
        }
        let from_at = self.p.tok.start;
        let (from, arg) = self.typed()?;
        self.p.expect_word(b"to", "expected 'to' after cast value")?;
        let to = self.p.parse_type()?;
        if !self.cast_is_valid(op, from, to) {
            return mistyped(from_at, "invalid cast opcode for cast between these types");
        }
        Ok((to, ir(at, self.b.cast(op, to, arg))?))
    }

    fn select(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        self.fast_math();
        let cond_at = self.p.tok.start;
        let (cond_ty, cond) = self.typed()?;
        self.comma()?;
        let (ty, if_true) = self.typed()?;
        self.comma()?;
        let other_at = self.p.tok.start;
        let (other, if_false) = self.typed()?;
        if other != ty {
            return mistyped(other_at, "select values must have same type");
        }
        let (cond_scalar, cond_shape) = self.shape(cond_ty);
        if cond_scalar != TypeId::I1 || (cond_shape.is_some() && cond_shape != self.shape(ty).1) {
            return mistyped(cond_at, "select condition must be i1 or <n x i1>");
        }
        Ok((ty, ir(at, self.b.select(ty, cond, if_true, if_false))?))
    }

    fn phi(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        self.fast_math();
        let ty_at = self.p.tok.start;
        let ty = self.p.parse_type()?;
        if matches!(
            self.p.module.types.kind(ty),
            TypeKind::Void | TypeKind::Label | TypeKind::Metadata | TypeKind::Function { .. }
        ) {
            return mistyped(ty_at, "phi node must have first class type");
        }
        self.s.incoming.clear();
        loop {
            self.p.expect(Tok::LBracket, "expected '[' in phi value list")?;
            let value = self.value(ty)?;
            self.p.expect(Tok::Comma, "expected ',' after insertelement value")?;
            let block = self.block_ref()?;
            self.p.expect(Tok::RBracket, "expected ']' in phi value list")?;
            self.s.incoming.push((value, block));
            if !self.p.eat(Tok::Comma) {
                break;
            }
            if self.p.tok.kind == Tok::Meta {
                self.extra_comma = true;
                break;
            }
        }
        Ok((ty, ir(at, self.b.phi(ty, &self.s.incoming))?))
    }

    fn va_arg(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let list = self.pointer("va_arg")?;
        self.p.expect(Tok::Comma, "expected ',' after vaarg operand")?;
        let ty_at = self.p.tok.start;
        let ty = self.p.parse_type()?;
        if !self.p.module.types.is_sized(ty) {
            return mistyped(ty_at, "va_arg requires operand with first class type");
        }
        Ok((ty, ir(at, self.b.va_arg(ty, list))?))
    }

    // ---- memory ----

    /// `align N`, at the word.
    fn alignment(&mut self) -> PResult<u64> {
        self.p.bump();
        let at = self.p.tok.start;
        let bytes = self.p.expect_u64("expected an alignment")?;
        if !bytes.is_power_of_two() {
            return fail(LlErrorKind::Syntax, at, "alignment is not a power of two");
        }
        if bytes > 1 << 32 {
            return fail(LlErrorKind::Syntax, at, "huge alignments are not supported yet");
        }
        Ok(bytes)
    }

    /// `{, align N}`, and the comma of the metadata that follows.
    fn comma_align(&mut self) -> PResult<Option<u64>> {
        let mut align = None;
        while self.p.eat(Tok::Comma) {
            if self.p.tok.kind == Tok::Meta {
                self.extra_comma = true;
                break;
            }
            if !self.p.at_word(b"align") {
                return self.p.syntax("expected metadata or 'align'");
            }
            align = Some(self.alignment()?);
        }
        Ok(align)
    }

    /// `[syncscope("name")]`.
    fn scope(&mut self) -> PResult<SyncScope> {
        if !self.p.eat_word(b"syncscope") {
            return Ok(SyncScope::System);
        }
        self.p.expect(Tok::LParen, "expected '(' in synchronization scope")?;
        let token = self.p.expect(Tok::Str, "synchronization scope name must be a string")?;
        self.p.expect(Tok::RParen, "expected ')' in synchronization scope")?;
        let name = self.p.string(token);
        Ok(match &*name {
            b"singlethread" => SyncScope::SingleThread,
            b"" => SyncScope::System,
            other => {
                // A scope of the target's own: `hod_ir` has the two.
                self.p.note(format!("syncscope:{}", String::from_utf8_lossy(other)));
                SyncScope::System
            }
        })
    }

    fn ordering(&mut self) -> PResult<AtomicOrdering> {
        let found = match self.p.tok.kind {
            Tok::Word => {
                let word = self.p.text();
                AtomicOrdering::ALL.iter().copied().find(|o| o.name().as_bytes() == word)
            }
            _ => None,
        };
        let Some(ordering) = found else {
            return self.p.syntax("expected ordering on atomic instruction");
        };
        self.p.bump();
        Ok(ordering)
    }

    /// The flags of an access: `align` bytes, or the default if `None`.
    fn mem_flags(
        at: u32,
        align: u64,
        volatile: bool,
        atomic: Option<(AtomicOrdering, SyncScope)>,
    ) -> PResult<MemFlags> {
        let mut mem = ir(at, MemFlags::aligned(align))?;
        if volatile {
            mem = mem.with_volatile();
        }
        if let Some((ordering, scope)) = atomic {
            mem = mem.with_atomic(ordering, scope);
        }
        Ok(mem)
    }

    fn load(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let atomic = self.p.eat_word(b"atomic");
        let volatile = self.p.eat_word(b"volatile");
        let ty_at = self.p.tok.start;
        let ty = self.p.parse_type()?;
        self.p.expect(Tok::Comma, "expected comma after load's type")?;
        let ptr = self.pointer("load")?;
        let mut order = None;
        if atomic {
            let scope = self.scope()?;
            let ordering_at = self.p.tok.start;
            let ordering = self.ordering()?;
            if !ordering.valid_for_load() {
                let message = "atomic load cannot use Release ordering";
                return fail(LlErrorKind::Syntax, ordering_at, message);
            }
            order = Some((ordering, scope));
        }
        let align = self.comma_align()?;
        let types = &self.p.module.types;
        if !types.is_sized(ty) || ty == TypeId::TOKEN {
            return mistyped(ty_at, "loading unsized types is not allowed");
        }
        if atomic && !self.atomic_type(ty) {
            return mistyped(ty_at, "atomic load operand must have a power-of-two byte size");
        }
        let align = align.unwrap_or_else(|| types.align_of(ty));
        let mem = Self::mem_flags(at, align, volatile, order)?;
        Ok((ty, ir(at, self.b.load(ty, ptr, mem))?))
    }

    fn store(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let atomic = self.p.eat_word(b"atomic");
        let volatile = self.p.eat_word(b"volatile");
        let ty_at = self.p.tok.start;
        let (ty, value) = self.typed()?;
        self.p.expect(Tok::Comma, "expected ',' after store operand")?;
        let ptr = self.pointer("store")?;
        let mut order = None;
        if atomic {
            let scope = self.scope()?;
            let ordering_at = self.p.tok.start;
            let ordering = self.ordering()?;
            if !ordering.valid_for_store() {
                let message = "atomic store cannot use Acquire ordering";
                return fail(LlErrorKind::Syntax, ordering_at, message);
            }
            order = Some((ordering, scope));
        }
        let align = self.comma_align()?;
        let types = &self.p.module.types;
        if !types.is_sized(ty) || ty == TypeId::TOKEN {
            return mistyped(ty_at, "storing unsized types is not allowed");
        }
        if atomic && !self.atomic_type(ty) {
            return mistyped(ty_at, "atomic store operand must have a power-of-two byte size");
        }
        let align = align.unwrap_or_else(|| types.align_of(ty));
        let mem = Self::mem_flags(at, align, volatile, order)?;
        ir(at, self.b.store(value, ptr, mem))?;
        Ok((TypeId::VOID, Value::NONE))
    }

    fn alloca(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let in_alloca = self.p.eat_word(b"inalloca");
        let swift_error = self.p.eat_word(b"swifterror");
        let ty_at = self.p.tok.start;
        let ty = self.p.parse_type()?;
        let mut count = None;
        let mut align = None;
        let mut space = self.p.module.types.data_layout().alloca_addr_space;
        // `[, type count] [, align N] [, addrspace(N)]`, and the comma
        // of the metadata that follows.
        let mut stage = 0;
        while self.p.eat(Tok::Comma) {
            if self.p.tok.kind == Tok::Meta {
                self.extra_comma = true;
                break;
            }
            if self.p.at_word(b"align") && stage < 2 {
                align = Some(self.alignment()?);
                stage = 2;
            } else if self.p.at_word(b"addrspace") && stage < 3 {
                self.p.bump();
                self.p.expect(Tok::LParen, "expected '(' in address space")?;
                space = self.p.expect_u32("expected an address space number")?;
                self.p.expect(Tok::RParen, "expected ')' in address space")?;
                stage = 3;
            } else if stage == 0 {
                let count_at = self.p.tok.start;
                let (count_ty, value) = self.typed()?;
                if !self.p.module.types.is_int(count_ty) {
                    return mistyped(count_at, "element count must have integer type");
                }
                count = Some(value);
                stage = 1;
            } else {
                return self.p.syntax("expected metadata or 'addrspace'");
            }
        }
        let types = &self.p.module.types;
        if !types.is_sized(ty) || ty == TypeId::TOKEN {
            return mistyped(ty_at, "invalid type for alloca");
        }
        let result = if space == 0 { TypeId::PTR } else { ir(at, self.p.module.types.ptr(space))? };
        let types = &self.p.module.types;
        if in_alloca {
            self.p.note("alloca:inalloca");
            return Ok((result, self.stand_in(at, result)?));
        }
        if swift_error {
            self.p.note("alloca:swifterror");
            return Ok((result, self.stand_in(at, result)?));
        }
        if types.is_scalable(ty) {
            self.p.note("alloca:scalable");
            return Ok((result, self.stand_in(at, result)?));
        }
        let elem_size = types.alloc_size_of(ty);
        let align = align.unwrap_or_else(|| types.pref_align_of(ty));
        // A static alloca is a slot (`spec/ir.md` §5.2): in the entry
        // block, with a count that is a literal.
        let literal = match count {
            None => Some(1),
            Some(value) => value.as_const().and_then(|id| self.p.module.consts.int_value(id)),
        };
        if self.blocks == 1
            && space == 0
            && let Some(size) = literal.and_then(|count| elem_size.checked_mul(count))
        {
            let slot = ir(at, self.b.add_slot(size, align))?;
            return Ok((result, ir(at, self.b.slot(slot))?));
        }
        let count = if let Some(count) = count {
            count
        } else {
            let module = &mut *self.p.module;
            let one = ir(at, module.consts.int(&module.types, TypeId::I32, 1))?;
            ir(at, Value::constant(one))?
        };
        Ok((result, ir(at, self.b.alloca(result, count, elem_size, align))?))
    }

    fn cmpxchg(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let weak = self.p.eat_word(b"weak");
        let volatile = self.p.eat_word(b"volatile");
        let ptr = self.pointer("cmpxchg")?;
        self.p.expect(Tok::Comma, "expected ',' after cmpxchg address")?;
        let ty_at = self.p.tok.start;
        let (ty, expected) = self.typed()?;
        self.p.expect(Tok::Comma, "expected ',' after cmpxchg cmp operand")?;
        let new_at = self.p.tok.start;
        let (new_ty, replacement) = self.typed()?;
        let scope = self.scope()?;
        let order_at = self.p.tok.start;
        let success = self.ordering()?;
        let failure_at = self.p.tok.start;
        let failure = self.ordering()?;
        let align = self.comma_align()?;
        if !success.valid_for_rmw() {
            return fail(LlErrorKind::Syntax, order_at, "invalid cmpxchg success ordering");
        }
        if !failure.valid_for_cmpxchg_failure() {
            return fail(LlErrorKind::Syntax, failure_at, "invalid cmpxchg failure ordering");
        }
        if new_ty != ty {
            return mistyped(new_at, "compare value and new value type do not match");
        }
        let types = &self.p.module.types;
        if !(types.is_int(ty) || types.is_ptr(ty)) || !self.atomic_type(ty) {
            let message =
                "cmpxchg operand must be an integer or pointer of a power-of-two byte size";
            return mistyped(ty_at, message);
        }
        let align = align.unwrap_or_else(|| types.size_of(ty).next_power_of_two());
        let mem = Self::mem_flags(at, align, volatile, Some((success, scope)))?;
        let result = ir(at, self.p.module.types.struct_type(&[ty, TypeId::I1], false))?;
        let value = self.b.cmpxchg(result, ptr, expected, replacement, mem, failure, weak);
        Ok((result, ir(at, value)?))
    }

    fn atomic_rmw(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let volatile = self.p.eat_word(b"volatile");
        let found = match self.p.tok.kind {
            Tok::Word => {
                let word = self.p.text();
                RmwOp::ALL.iter().copied().find(|op| op.name().as_bytes() == word)
            }
            _ => None,
        };
        let Some(op) = found else {
            return self.p.syntax("expected binary operation in atomicrmw");
        };
        self.p.bump();
        let ptr = self.pointer("atomicrmw")?;
        self.p.expect(Tok::Comma, "expected ',' after atomicrmw address")?;
        let ty_at = self.p.tok.start;
        let (ty, value) = self.typed()?;
        let scope = self.scope()?;
        let order_at = self.p.tok.start;
        let ordering = self.ordering()?;
        let align = self.comma_align()?;
        if !ordering.valid_for_rmw() {
            return fail(LlErrorKind::Syntax, order_at, "atomicrmw cannot be unordered");
        }
        let types = &self.p.module.types;
        let (scalar, shape) = self.shape(ty);
        let class = match op {
            RmwOp::Xchg => types.is_int(ty) || types.is_float(ty) || types.is_ptr(ty),
            op if op.is_float() => {
                types.is_float(scalar) && !shape.is_some_and(|(_, scalable)| scalable)
            }
            _ => types.is_int(ty),
        };
        if !class || !self.atomic_type(ty) {
            return mistyped(ty_at, "atomicrmw operand has a type the operation does not take");
        }
        let align = align.unwrap_or_else(|| types.size_of(ty).next_power_of_two());
        let mem = Self::mem_flags(at, align, volatile, Some((ordering, scope)))?;
        Ok((ty, ir(at, self.b.atomic_rmw(op, ty, ptr, value, mem))?))
    }

    fn fence(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let scope = self.scope()?;
        let order_at = self.p.tok.start;
        let ordering = self.ordering()?;
        if !ordering.valid_for_fence() {
            let message = "fence cannot be unordered or monotonic";
            return fail(LlErrorKind::Syntax, order_at, message);
        }
        ir(at, self.b.fence(ordering, scope))?;
        Ok((TypeId::VOID, Value::NONE))
    }

    // ---- addresses and aggregates ----

    /// The value of a constant used as a `getelementptr` index:
    /// sign-extended or truncated to 64 bits. `None` if it is not one
    /// integer (the same in every element, for a vector).
    fn index_value(&self, id: ConstId) -> Option<i64> {
        let module = &*self.p.module;
        match module.consts.get(id) {
            Const::Int { ty, value } => {
                Some(sext64(value, module.types.int_bits(ty).unwrap_or(64)))
            }
            Const::WideInt { words, .. } => Some(words[0] as i64),
            Const::Zero(_) => Some(0),
            Const::Splat { elem, .. } => self.index_value(elem),
            Const::Aggregate { elems, .. } => {
                let first = self.index_value(*elems.first()?)?;
                elems[1..]
                    .iter()
                    .all(|&elem| self.index_value(elem) == Some(first))
                    .then_some(first)
            }
            _ => None,
        }
    }

    /// `getelementptr`: a `PtrAdd` (`spec/ir.md` §5.4) with every
    /// constant index folded into the offset.
    #[allow(clippy::too_many_lines)] // One grammar, read in one place.
    fn get_element_ptr(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        self.flags(&[b"inbounds", b"nuw", b"nusw"]);
        let source_at = self.p.tok.start;
        let source = self.p.parse_type()?;
        self.p.expect(Tok::Comma, "expected comma after getelementptr's type")?;
        let base_at = self.p.tok.start;
        let (base_ty, base) = self.typed()?;
        let (pointer, mut shape) = self.shape(base_ty);
        if !self.p.module.types.is_ptr(pointer) {
            return mistyped(base_at, "base of getelementptr must be a pointer");
        }
        if !self.p.module.types.is_sized(source) {
            return mistyped(source_at, "base element of getelementptr must be sized");
        }
        self.s.indices.clear();
        let mut offset = 0i64;
        let mut current = source;
        let mut first = true;
        // The reason the address cannot be a `PtrAdd`, if there is one.
        let mut unsupported = None;
        while self.p.eat(Tok::Comma) {
            if self.p.tok.kind == Tok::Meta {
                self.extra_comma = true;
                break;
            }
            let index_at = self.p.tok.start;
            let index_ty = self.p.parse_type()?;
            let (index_scalar, index_shape) = self.shape(index_ty);
            let Some(bits) = self.p.module.types.int_bits(index_scalar) else {
                return mistyped(index_at, "getelementptr index must be an integer");
            };
            if index_shape.is_some() {
                if shape.is_some() && shape != index_shape {
                    let message = "getelementptr vector index has a wrong number of elements";
                    return mistyped(index_at, message);
                }
                shape = index_shape;
            }
            // The index: its value if it is a constant.
            let token = self.p.tok;
            let (value, constant) =
                if token.kind == Tok::Int && index_shape.is_none() && token.end - token.start <= 18
                {
                    let text = self.p.tok_text(token);
                    let (negative, digits) = match text.strip_prefix(b"-") {
                        Some(digits) => (true, digits),
                        None => (false, text),
                    };
                    let magnitude = digits.iter().fold(0i64, |v, &d| v * 10 + i64::from(d - b'0'));
                    let literal = if negative { -magnitude } else { magnitude };
                    self.p.bump();
                    (Value::NONE, Some(sext64(literal as u64, bits)))
                } else {
                    let value = self.value(index_ty)?;
                    (value, value.as_const().and_then(|id| self.index_value(id)))
                };
            let types = &self.p.module.types;
            let stride = if first {
                first = false;
                (!types.is_scalable(current)).then(|| types.alloc_size_of(current))
            } else {
                match types.kind(current) {
                    TypeKind::Struct { fields, .. } => {
                        let field = constant.and_then(|index| {
                            usize::try_from(index).ok().filter(|&index| index < fields.len())
                        });
                        let Some(field) = field else {
                            return mistyped(index_at, "invalid getelementptr indices");
                        };
                        let field_ty = fields[field];
                        if types.is_scalable(current) {
                            unsupported = Some("getelementptr:scalable");
                        } else {
                            let at_field = types.field_offset(current, field as u32) as i64;
                            offset = offset.wrapping_add(at_field);
                        }
                        current = field_ty;
                        continue;
                    }
                    TypeKind::Array { elem, .. } => {
                        current = elem;
                        (!types.is_scalable(elem)).then(|| types.alloc_size_of(elem))
                    }
                    TypeKind::Vector { elem, .. } => {
                        let stride = types.element_stride(current);
                        if stride.is_none() {
                            unsupported = Some("getelementptr:vector-element-not-bytes");
                        }
                        current = elem;
                        stride.or(Some(0))
                    }
                    TypeKind::ScalableVector { elem, .. } => {
                        current = elem;
                        None
                    }
                    _ => return mistyped(index_at, "invalid getelementptr indices"),
                }
            };
            let Some(stride) = stride else {
                unsupported = Some("getelementptr:scalable");
                continue;
            };
            match constant {
                Some(index) => offset = offset.wrapping_add(index.wrapping_mul(stride as i64)),
                None => self.s.indices.push((value, stride)),
            }
        }
        let result = self.shaped(at, pointer, shape)?;
        if let Some(reason) = unsupported {
            self.p.note(reason);
            return Ok((result, self.stand_in(at, result)?));
        }
        Ok((result, ir(at, self.b.ptr_add(result, base, offset, &self.s.indices))?))
    }

    /// `, index {, index}` into `s.path`, and the comma of the metadata
    /// that follows.
    fn index_path(&mut self) -> PResult<()> {
        self.s.path.clear();
        if self.p.tok.kind != Tok::Comma {
            return self.p.syntax("expected ',' as start of index list");
        }
        while self.p.eat(Tok::Comma) {
            if self.p.tok.kind == Tok::Meta {
                if self.s.path.is_empty() {
                    return self.p.syntax("expected index");
                }
                self.extra_comma = true;
                break;
            }
            let index = self.p.expect_u32("expected index")?;
            self.s.path.push(index);
        }
        Ok(())
    }

    /// The type of the field of `agg` at `s.path`.
    fn field_type(&self, agg: TypeId) -> Option<TypeId> {
        let types = &self.p.module.types;
        let mut ty = agg;
        for &index in &self.s.path {
            ty = match types.kind(ty) {
                TypeKind::Struct { fields, .. } => *fields.get(index as usize)?,
                TypeKind::Array { elem, len } if u64::from(index) < len => elem,
                _ => return None,
            };
        }
        Some(ty)
    }

    fn extract_value(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let agg_at = self.p.tok.start;
        let (agg_ty, agg) = self.typed()?;
        self.index_path()?;
        let Some(field) = self.field_type(agg_ty) else {
            return mistyped(agg_at, "invalid indices for extractvalue");
        };
        if let Ok((offset, _)) = aggregate_field(&self.p.module.types, agg_ty, &self.s.path) {
            return Ok((field, ir(at, self.b.extract_value(field, agg, offset))?));
        }
        // A field after a scalable vector has no fixed offset.
        self.p.note("aggregate:no-fixed-offset");
        Ok((field, self.stand_in(at, field)?))
    }

    fn insert_value(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let agg_at = self.p.tok.start;
        let (agg_ty, agg) = self.typed()?;
        self.p.expect(Tok::Comma, "expected comma after insertvalue operand")?;
        let value_at = self.p.tok.start;
        let (value_ty, value) = self.typed()?;
        self.index_path()?;
        let Some(field) = self.field_type(agg_ty) else {
            return mistyped(agg_at, "invalid indices for insertvalue");
        };
        if field != value_ty {
            return mistyped(value_at, "insertvalue operand and field disagree in type");
        }
        match aggregate_field(&self.p.module.types, agg_ty, &self.s.path) {
            Ok((offset, _)) if u32::try_from(offset).is_ok() => {
                Ok((agg_ty, ir(at, self.b.insert_value(agg_ty, agg, value, offset))?))
            }
            Ok(_) => {
                self.p.note("insertvalue:offset-over-4GiB");
                Ok((agg_ty, self.stand_in(at, agg_ty)?))
            }
            Err(_) => {
                self.p.note("aggregate:no-fixed-offset");
                Ok((agg_ty, self.stand_in(at, agg_ty)?))
            }
        }
    }

    fn extract_element(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let vector_at = self.p.tok.start;
        let (vector_ty, vector) = self.typed()?;
        self.p.expect(Tok::Comma, "expected ',' after extract value")?;
        let (index_ty, index) = self.typed()?;
        let (elem, shape) = self.shape(vector_ty);
        if shape.is_none() || !self.p.module.types.is_int(index_ty) {
            return mistyped(vector_at, "invalid extractelement operands");
        }
        Ok((elem, ir(at, self.b.extract_element(elem, vector, index))?))
    }

    fn insert_element(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let vector_at = self.p.tok.start;
        let (vector_ty, vector) = self.typed()?;
        self.p.expect(Tok::Comma, "expected ',' after insertelement value")?;
        let (value_ty, value) = self.typed()?;
        self.p.expect(Tok::Comma, "expected ',' after insertelement value")?;
        let (index_ty, index) = self.typed()?;
        let (elem, shape) = self.shape(vector_ty);
        if shape.is_none() || value_ty != elem || !self.p.module.types.is_int(index_ty) {
            return mistyped(vector_at, "invalid insertelement operands");
        }
        Ok((vector_ty, ir(at, self.b.insert_element(vector_ty, vector, value, index))?))
    }

    fn shuffle_vector(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let first_at = self.p.tok.start;
        let (ty, first) = self.typed()?;
        self.p.expect(Tok::Comma, "expected ',' after shuffle mask")?;
        let (second_ty, second) = self.typed()?;
        self.p.expect(Tok::Comma, "expected ',' after shuffle value")?;
        let mask = self.p.parse_typed_const()?;
        let invalid = "invalid shufflevector operands";
        let (elem, Some((len, scalable))) = self.shape(ty) else {
            return mistyped(first_at, invalid);
        };
        let module = &*self.p.module;
        let mask_ty = module.consts.ty(mask);
        let (mask_len, mask_scalable) = match module.types.kind(mask_ty) {
            TypeKind::Vector { elem: TypeId::I32, len } => (len, false),
            TypeKind::ScalableVector { elem: TypeId::I32, min_len } => (min_len, true),
            _ => return mistyped(first_at, invalid),
        };
        if second_ty != ty || mask_scalable != scalable {
            return mistyped(first_at, invalid);
        }
        let entry = |id: ConstId| match module.consts.get(id) {
            Const::Int { value, .. } => u32::try_from(value).ok().filter(|&m| m != SHUFFLE_UNDEF),
            Const::Zero(_) => Some(0),
            Const::Undef(_) | Const::Poison(_) => Some(SHUFFLE_UNDEF),
            _ => None,
        };
        let out = &mut self.s.mask;
        out.clear();
        let complete = match module.consts.get(mask) {
            Const::Zero(_) => {
                out.resize(mask_len as usize, 0);
                true
            }
            Const::Undef(_) | Const::Poison(_) => {
                out.resize(mask_len as usize, SHUFFLE_UNDEF);
                true
            }
            Const::Splat { elem, .. } => entry(elem).is_some_and(|m| {
                out.resize(mask_len as usize, m);
                true
            }),
            Const::Aggregate { elems, .. } => elems.iter().all(|&elem| {
                entry(elem).is_some_and(|m| {
                    out.push(m);
                    true
                })
            }),
            _ => false,
        };
        let limit = if scalable { 1 } else { 2 * u64::from(len) };
        if !complete || out.iter().any(|&m| m != SHUFFLE_UNDEF && u64::from(m) >= limit) {
            return mistyped(first_at, invalid);
        }
        let result = self.shaped(at, elem, Some((mask_len, scalable)))?;
        Ok((result, ir(at, self.b.shuffle_vector(result, first, second, &self.s.mask))?))
    }

    // ---- exceptions ----

    fn landing_pad(&mut self, at: u32) -> PResult<(TypeId, Value)> {
        let ty = self.p.parse_type()?;
        let cleanup = self.p.eat_word(b"cleanup");
        self.s.clauses.clear();
        loop {
            let catch = self.p.at_word(b"catch");
            if !catch && !self.p.at_word(b"filter") {
                break;
            }
            self.p.bump();
            let clause_at = self.p.tok.start;
            let constant = self.p.parse_typed_const()?;
            let module = &*self.p.module;
            let clause_ty = module.consts.ty(constant);
            let array = matches!(module.types.kind(clause_ty), TypeKind::Array { .. });
            self.s.clauses.push(match (catch, array) {
                (true, false) if module.types.is_ptr(clause_ty) => Clause::Catch(constant),
                (false, true) => Clause::Filter(constant),
                (true, _) => return mistyped(clause_at, "'catch' clause has an invalid type"),
                (false, _) => return mistyped(clause_at, "'filter' clause has an invalid type"),
            });
        }
        if !cleanup && self.s.clauses.is_empty() {
            let message = "landingpad needs at least one clause or to be a cleanup";
            return fail(LlErrorKind::Syntax, at, message);
        }
        Ok((ty, ir(at, self.b.landing_pad(ty, cleanup, &self.s.clauses))?))
    }

    /// `within none` or `within %pad`.
    fn within(&mut self) -> PResult<()> {
        self.p.expect_word(b"within", "expected 'within' after the funclet instruction")?;
        self.pad_ref()
    }

    /// `none` or a token value.
    fn pad_ref(&mut self) -> PResult<()> {
        if self.p.eat_word(b"none") {
            return Ok(());
        }
        let token = self.p.tok;
        if !matches!(token.kind, Tok::Local | Tok::LocalQuoted | Tok::LocalId) {
            return self.p.syntax("expected scope value for the funclet instruction");
        }
        self.p.bump();
        self.local_value(token, TypeId::TOKEN).map(drop)
    }

    /// `[ type value, … ]`: the arguments of a pad.
    fn pad_args(&mut self) -> PResult<()> {
        self.p.expect(Tok::LBracket, "expected '[' in catchpad/cleanuppad")?;
        let mut first = true;
        while self.p.tok.kind != Tok::RBracket {
            if !first {
                self.p.expect(Tok::Comma, "expected ',' in argument list")?;
            }
            first = false;
            let ty = self.p.parse_type()?;
            if ty == TypeId::METADATA {
                self.metadata_operand()?;
            } else {
                self.value(ty)?;
            }
        }
        self.p.bump();
        Ok(())
    }

    /// `catchpad` or `cleanuppad`. The funclet instructions exist in
    /// `hod_ir` as opcodes only (`spec/ir.md` §5.4): the function is
    /// unsupported.
    fn pad(&mut self, at: u32, op: Opcode) -> PResult<(TypeId, Value)> {
        self.within()?;
        self.pad_args()?;
        self.p.note(if op == Opcode::CatchPad { "inst:catchpad" } else { "inst:cleanuppad" });
        Ok((TypeId::TOKEN, ir(at, self.b.funclet(op, TypeId::TOKEN))?))
    }

    /// `unwind to caller` or `unwind label %block`.
    fn unwind(&mut self) -> PResult<()> {
        self.p.expect_word(b"unwind", "expected 'unwind'")?;
        if self.p.eat_word(b"to") {
            return self.p.expect_word(b"caller", "expected 'caller' in the unwind destination");
        }
        self.label().map(drop)
    }

    fn catch_switch(&mut self, at: u32) -> PResult<Done> {
        self.within()?;
        self.p.expect(Tok::LBracket, "expected '[' with catchswitch labels")?;
        loop {
            self.label()?;
            if !self.p.eat(Tok::Comma) {
                break;
            }
        }
        self.p.expect(Tok::RBracket, "expected ']' after catchswitch labels")?;
        self.unwind()?;
        self.p.note("inst:catchswitch");
        let value = ir(at, self.b.funclet(Opcode::CatchSwitch, TypeId::TOKEN))?;
        Ok((TypeId::TOKEN, value, true))
    }

    fn catch_ret(&mut self, at: u32) -> PResult<Done> {
        self.p.expect_word(b"from", "expected 'from' after catchret")?;
        self.pad_ref()?;
        self.p.expect_word(b"to", "expected 'to' in catchret")?;
        self.label()?;
        self.p.note("inst:catchret");
        ir(at, self.b.funclet(Opcode::CatchRet, TypeId::VOID))?;
        Ok((TypeId::VOID, Value::NONE, true))
    }

    fn cleanup_ret(&mut self, at: u32) -> PResult<Done> {
        self.p.expect_word(b"from", "expected 'from' after cleanupret")?;
        self.pad_ref()?;
        self.unwind()?;
        self.p.note("inst:cleanupret");
        ir(at, self.b.funclet(Opcode::CleanupRet, TypeId::VOID))?;
        Ok((TypeId::VOID, Value::NONE, true))
    }

    // ---- terminators ----

    fn ret(&mut self, at: u32) -> PResult<Done> {
        let ty_at = self.p.tok.start;
        let ty = self.p.parse_type()?;
        let value = if ty == TypeId::VOID { None } else { Some(self.value(ty)?) };
        if ty != self.ret_ty {
            return mistyped(ty_at, "value doesn't match function result type");
        }
        ir(at, self.b.ret(value))?;
        Ok((TypeId::VOID, Value::NONE, true))
    }

    fn br(&mut self, at: u32) -> PResult<Done> {
        if self.p.at_word(b"label") {
            let target = self.label()?;
            ir(at, self.b.br(target))?;
            return Ok((TypeId::VOID, Value::NONE, true));
        }
        let cond_at = self.p.tok.start;
        let (ty, cond) = self.typed()?;
        if ty != TypeId::I1 {
            return mistyped(cond_at, "branch condition must have 'i1' type");
        }
        self.p.expect(Tok::Comma, "expected ',' after branch condition")?;
        let if_true = self.label()?;
        self.p.expect(Tok::Comma, "expected ',' after true destination")?;
        let if_false = self.label()?;
        ir(at, self.b.cond_br(cond, if_true, if_false))?;
        Ok((TypeId::VOID, Value::NONE, true))
    }

    fn switch(&mut self, at: u32) -> PResult<Done> {
        let cond_at = self.p.tok.start;
        let (ty, cond) = self.typed()?;
        if !self.p.module.types.is_int(ty) {
            return mistyped(cond_at, "switch condition must have integer type");
        }
        self.p.expect(Tok::Comma, "expected ',' after switch condition")?;
        let default = self.label()?;
        self.p.expect(Tok::LBracket, "expected '[' with switch table")?;
        self.s.cases.clear();
        self.s.case_ids.clear();
        while self.p.tok.kind != Tok::RBracket {
            let case_at = self.p.tok.start;
            let constant = self.p.parse_typed_const()?;
            let module = &*self.p.module;
            if module.consts.ty(constant) != ty {
                return mistyped(case_at, "case value is not of the switch condition's type");
            }
            if !matches!(module.consts.get(constant), Const::Int { .. } | Const::WideInt { .. }) {
                return mistyped(case_at, "case value is not a constant integer");
            }
            self.p.expect(Tok::Comma, "expected ',' after case value")?;
            let target = self.label()?;
            self.s.cases.push((constant, target));
            self.s.case_ids.push(constant.raw());
        }
        self.p.bump();
        // Constants are interned: equal values are equal ids.
        self.s.case_ids.sort_unstable();
        if self.s.case_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return fail(LlErrorKind::Syntax, at, "duplicate case value in switch");
        }
        ir(at, self.b.switch(cond, default, &self.s.cases))?;
        Ok((TypeId::VOID, Value::NONE, true))
    }

    fn indirect_br(&mut self, at: u32) -> PResult<Done> {
        let address = self.pointer("indirectbr")?;
        self.p.expect(Tok::Comma, "expected ',' after indirectbr address")?;
        self.p.expect(Tok::LBracket, "expected '[' with indirectbr")?;
        self.s.targets.clear();
        if self.p.tok.kind != Tok::RBracket {
            loop {
                let target = self.label()?;
                self.s.targets.push(target);
                if !self.p.eat(Tok::Comma) {
                    break;
                }
            }
        }
        self.p.expect(Tok::RBracket, "expected ']' at end of block list")?;
        ir(at, self.b.indirect_br(address, &self.s.targets))?;
        Ok((TypeId::VOID, Value::NONE, true))
    }
}
