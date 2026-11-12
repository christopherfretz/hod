//! Instruction records (`spec/bc.md` §11.4, §11.5): the operand layout of
//! each as LLVM 19 to 22 write it, the checks `hod_ll` makes on the same
//! instruction as text, and its form in `hod_ir`.

use hod_ir::{
    AtomicOrdering, BlockId, Bundle, CallConv, CallDesc, Clause, Const, ConstId, FloatPredicate,
    FnAttrs, InlineAsm, IntPredicate, Intrinsic, MemFlags, NoCode, Opcode, ParamAttrs, RmwOp,
    SHUFFLE_UNDEF, SymId, SyncScope, TailKind, TypeId, TypeKind, Value, aggregate_field,
    no_code_intrinsic, resolve_intrinsic,
};

use crate::attrs::{self, FUNCTION_INDEX};
use crate::body::{CalleeInfo, Fx, Local, Ops};
use crate::codes::func;
use crate::error::{BcErrorKind, BcResult, fail, ir};

/// What an instruction leaves: its result type and value (`void` and no
/// value for most), and whether it ended the block.
pub(crate) type Done = (TypeId, Value, bool);

/// The length of a vector type and whether it is scalable.
type Shape = Option<(u32, bool)>;

/// `value` sign-extended from `bits` bits.
fn sext64(value: u64, bits: u32) -> i64 {
    if bits >= 64 {
        value as i64
    } else {
        let shift = 64 - bits;
        ((value << shift) as i64) >> shift
    }
}

/// What is called.
#[derive(Clone, Copy)]
enum Target {
    Sym(SymId),
    Value(Value),
    /// Inline assembly: the function constant's id.
    Asm(u64),
}

/// The three kinds of call record.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Call,
    Invoke,
    CallBr,
}

impl Fx<'_, '_, '_> {
    // ---- types ----

    fn shape(&self, ty: TypeId) -> (TypeId, Shape) {
        match self.module.types.kind(ty) {
            TypeKind::Vector { elem, len } => (elem, Some((len, false))),
            TypeKind::ScalableVector { elem, min_len } => (elem, Some((min_len, true))),
            _ => (ty, None),
        }
    }

    fn shaped(&mut self, elem: TypeId, shape: Shape) -> BcResult<TypeId> {
        let types = &mut self.module.types;
        match shape {
            None => Ok(elem),
            Some((len, false)) => ir(self.at, types.vector(elem, len)),
            Some((len, true)) => ir(self.at, types.scalable_vector(elem, len)),
        }
    }

    /// What stands in for the result of an instruction `hod_ir` cannot
    /// represent, in a body that will not be kept.
    fn stand_in(&mut self, ty: TypeId) -> BcResult<Value> {
        let module = &mut *self.module;
        let id = match ty {
            TypeId::VOID => return Ok(Value::NONE),
            TypeId::TOKEN => module.consts.none_token(),
            _ => module.consts.poison(&module.types, ty),
        };
        ir(self.at, Value::constant(ir(self.at, id)?))
    }

    fn atomic_type(&self, ty: TypeId) -> bool {
        let types = &self.module.types;
        let (scalar, shape) = self.shape(ty);
        let class = types.is_int(scalar) || types.is_ptr(scalar) || types.is_float(scalar);
        let bits = types.size_in_bits_of(scalar) * shape.map_or(1, |(len, _)| u64::from(len));
        class && bits >= 8 && bits.is_power_of_two()
    }

    /// LLVM's `castIsValid`, as `hod_ll` has it.
    fn cast_is_valid(&self, op: Opcode, src: TypeId, dst: TypeId) -> bool {
        let types = &self.module.types;
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

    fn pointer(&mut self, o: &mut Ops<'_>, what: &str) -> BcResult<Local> {
        let local = self.vt(o)?;
        if !self.module.types.is_ptr(local.ty) {
            return self.mistyped(&format!("{what} operand must be a pointer"));
        }
        Ok(local)
    }

    // ---- the instruction ----

    /// One instruction record.
    pub(crate) fn instruction(&mut self, code: u32, o: &mut Ops<'_>) -> BcResult<Done> {
        let (ty, value) = match code {
            func::BINOP => self.binary(o)?,
            func::UNOP => self.unary(o)?,
            func::CAST => self.cast(o)?,
            func::GEP => self.get_element_ptr(o)?,
            func::EXTRACTVAL => self.extract_value(o)?,
            func::INSERTVAL => self.insert_value(o)?,
            func::VSELECT => self.select(o)?,
            func::EXTRACTELT => self.extract_element(o)?,
            func::INSERTELT => self.insert_element(o)?,
            func::SHUFFLEVEC => self.shuffle_vector(o)?,
            func::CMP2 => self.compare(o)?,
            func::PHI => self.phi(o)?,
            func::ALLOCA => self.alloca(o)?,
            func::LOAD | func::LOADATOMIC => self.load(o, code == func::LOADATOMIC)?,
            func::STORE | func::STOREATOMIC => self.store(o, code == func::STOREATOMIC)?,
            func::CMPXCHG => self.cmpxchg(o)?,
            func::ATOMICRMW => self.atomic_rmw(o)?,
            func::FENCE => self.fence(o)?,
            func::VAARG => self.va_arg(o)?,
            func::FREEZE => {
                let arg = self.vt(o)?;
                (arg.ty, ir(self.at, self.b.freeze(arg.ty, arg.value))?)
            }
            func::LANDINGPAD => self.landing_pad(o)?,
            func::CALL => return self.call(o, Kind::Call),
            func::INVOKE => return self.call(o, Kind::Invoke),
            func::CALLBR => return self.call(o, Kind::CallBr),
            func::RET => return self.ret(o),
            func::BR => return self.br(o),
            func::SWITCH => return self.switch(o),
            func::INDIRECTBR => return self.indirect_br(o),
            func::RESUME => {
                let value = self.vt(o)?;
                ir(self.at, self.b.resume(value.value))?;
                return Ok((TypeId::VOID, Value::NONE, true));
            }
            func::UNREACHABLE => {
                ir(self.at, self.b.unreachable())?;
                return Ok((TypeId::VOID, Value::NONE, true));
            }
            func::CATCHPAD | func::CLEANUPPAD => {
                let (op, name) = if code == func::CATCHPAD {
                    (Opcode::CatchPad, "inst:catchpad")
                } else {
                    (Opcode::CleanupPad, "inst:cleanuppad")
                };
                self.note(name);
                (TypeId::TOKEN, ir(self.at, self.b.funclet(op, TypeId::TOKEN))?)
            }
            func::CATCHSWITCH => {
                self.note("inst:catchswitch");
                let value = ir(self.at, self.b.funclet(Opcode::CatchSwitch, TypeId::TOKEN))?;
                return Ok((TypeId::TOKEN, value, true));
            }
            func::CATCHRET | func::CLEANUPRET => {
                let (op, name) = if code == func::CATCHRET {
                    (Opcode::CatchRet, "inst:catchret")
                } else {
                    (Opcode::CleanupRet, "inst:cleanupret")
                };
                self.note(name);
                ir(self.at, self.b.funclet(op, TypeId::VOID))?;
                return Ok((TypeId::VOID, Value::NONE, true));
            }
            _ => {
                return fail(
                    BcErrorKind::Unsupported("record"),
                    self.at,
                    format!("record code {code} in a FUNCTION block"),
                );
            }
        };
        Ok((ty, value, false))
    }

    // ---- arithmetic, comparisons, casts ----

    fn binary(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let lhs = self.vt(o)?;
        let rhs = self.v(o, lhs.ty)?;
        let code = o.next()?;
        let ty = lhs.ty;
        let scalar = self.shape(ty).0;
        let float = self.module.types.is_float(scalar);
        let op = match (code, float) {
            (0, false) => Opcode::Add,
            (0, true) => Opcode::FAdd,
            (1, false) => Opcode::Sub,
            (1, true) => Opcode::FSub,
            (2, false) => Opcode::Mul,
            (2, true) => Opcode::FMul,
            (3, false) => Opcode::UDiv,
            (4, false) => Opcode::SDiv,
            (4, true) => Opcode::FDiv,
            (5, false) => Opcode::URem,
            (6, false) => Opcode::SRem,
            (6, true) => Opcode::FRem,
            (7, false) => Opcode::Shl,
            (8, false) => Opcode::LShr,
            (9, false) => Opcode::AShr,
            (10, false) => Opcode::And,
            (11, false) => Opcode::Or,
            (12, false) => Opcode::Xor,
            _ => return self.mistyped("invalid operand type for instruction"),
        };
        if !float && !self.module.types.is_int(scalar) {
            return self.mistyped("invalid operand type for instruction");
        }
        Ok((ty, ir(self.at, self.b.binary(op, ty, lhs.value, rhs.value))?))
    }

    fn unary(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let arg = self.vt(o)?;
        if o.next()? != 0 {
            return self.invalid("a unary operator other than fneg");
        }
        if !self.module.types.is_float(self.shape(arg.ty).0) {
            return self.mistyped("invalid operand type for instruction");
        }
        Ok((arg.ty, ir(self.at, self.b.fneg(arg.ty, arg.value))?))
    }

    fn compare(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let lhs = self.vt(o)?;
        let rhs = self.v(o, lhs.ty)?;
        let pred = o.next()?;
        let (scalar, shape) = self.shape(lhs.ty);
        let result = self.shaped(TypeId::I1, shape)?;
        let types = &self.module.types;
        if types.is_float(scalar) {
            let Some(&pred) = usize::try_from(pred).ok().and_then(|p| FloatPredicate::ALL.get(p))
            else {
                return self.invalid("an fcmp predicate out of range");
            };
            return Ok((result, ir(self.at, self.b.fcmp(pred, result, lhs.value, rhs.value))?));
        }
        if !types.is_int(scalar) && !types.is_ptr(scalar) {
            return self.mistyped("icmp requires integer operands");
        }
        let found = pred
            .checked_sub(32)
            .and_then(|p| usize::try_from(p).ok())
            .and_then(|p| IntPredicate::ALL.get(p));
        let Some(&pred) = found else {
            return self.invalid("an icmp predicate out of range");
        };
        Ok((result, ir(self.at, self.b.icmp(pred, result, lhs.value, rhs.value))?))
    }

    fn cast(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let arg = self.vt(o)?;
        let to = self.ty(o.next()?)?;
        let op = match o.next()? {
            0 => Opcode::Trunc,
            1 => Opcode::ZExt,
            2 => Opcode::SExt,
            3 => Opcode::FPToUI,
            4 => Opcode::FPToSI,
            5 => Opcode::UIToFP,
            6 => Opcode::SIToFP,
            7 => Opcode::FPTrunc,
            8 => Opcode::FPExt,
            9 => Opcode::PtrToInt,
            10 => Opcode::IntToPtr,
            11 => Opcode::BitCast,
            12 => Opcode::AddrSpaceCast,
            13 => Opcode::PtrToAddr,
            _ => return self.invalid("a cast opcode out of range"),
        };
        if !self.cast_is_valid(op, arg.ty, to) {
            return self.mistyped("invalid cast opcode for cast between these types");
        }
        Ok((to, ir(self.at, self.b.cast(op, to, arg.value))?))
    }

    fn select(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let if_true = self.vt(o)?;
        let if_false = self.v(o, if_true.ty)?;
        let cond = self.vt(o)?;
        let ty = if_true.ty;
        let (cond_scalar, cond_shape) = self.shape(cond.ty);
        if cond_scalar != TypeId::I1 || (cond_shape.is_some() && cond_shape != self.shape(ty).1) {
            return self.mistyped("select condition must be i1 or <n x i1>");
        }
        Ok((ty, ir(self.at, self.b.select(ty, cond.value, if_true.value, if_false.value))?))
    }

    fn phi(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let ty = self.ty(o.next()?)?;
        if matches!(
            self.module.types.kind(ty),
            TypeKind::Void | TypeKind::Label | TypeKind::Metadata | TypeKind::Function { .. }
        ) {
            return self.mistyped("phi node must have first class type");
        }
        let mut incoming = std::mem::take(&mut self.s.incoming);
        incoming.clear();
        let pairs = o.left() / 2;
        let mut result = Ok(());
        for _ in 0..pairs {
            let value = match self.v_signed(o, ty) {
                Ok(value) => value,
                Err(error) => {
                    result = Err(error);
                    break;
                }
            };
            match o.next().and_then(|id| self.block(id)) {
                Ok(block) => incoming.push((value.value, block)),
                Err(error) => {
                    result = Err(error);
                    break;
                }
            }
        }
        let built = result.and_then(|()| ir(self.at, self.b.phi(ty, &incoming)));
        self.s.incoming = incoming;
        Ok((ty, built?))
    }

    fn va_arg(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let list_ty = self.ty(o.next()?)?;
        let list = self.v(o, list_ty)?;
        let ty = self.ty(o.next()?)?;
        if !self.module.types.is_ptr(list_ty) {
            return self.mistyped("va_arg operand must be a pointer");
        }
        if !self.module.types.is_sized(ty) {
            return self.mistyped("va_arg requires operand with first class type");
        }
        Ok((ty, ir(self.at, self.b.va_arg(ty, list.value))?))
    }

    // ---- memory ----

    /// An encoded alignment: log2 + 1, 0 for none.
    fn alignment(&self, encoded: u64) -> BcResult<Option<u64>> {
        match encoded {
            0 => Ok(None),
            1..=33 => Ok(Some(1 << (encoded - 1))),
            _ => self.invalid("huge alignments are not supported yet"),
        }
    }

    fn ordering(&self, code: u64) -> BcResult<AtomicOrdering> {
        match usize::try_from(code).ok().and_then(|c| c.checked_sub(1)) {
            Some(index) if index < AtomicOrdering::ALL.len() => Ok(AtomicOrdering::ALL[index]),
            _ => self.invalid("an atomic ordering out of range"),
        }
    }

    /// Sync scope `id`, by the module's SYNC_SCOPE_NAMES.
    fn scope(&mut self, id: u64) -> BcResult<SyncScope> {
        let names = self.bits.sync_scopes();
        if names.is_empty() {
            return match id {
                0 => Ok(SyncScope::SingleThread),
                1 => Ok(SyncScope::System),
                _ => self.invalid("a sync scope that is not defined"),
            };
        }
        let Some(name) = usize::try_from(id).ok().and_then(|i| names.get(i)) else {
            return self.invalid("a sync scope that is not defined");
        };
        Ok(match &name[..] {
            b"singlethread" => SyncScope::SingleThread,
            b"" => SyncScope::System,
            other => {
                // A scope of the target's own: `hod_ir` has the two.
                self.note(format!("syncscope:{}", String::from_utf8_lossy(other)));
                SyncScope::System
            }
        })
    }

    fn mem_flags(
        &self,
        align: u64,
        volatile: bool,
        atomic: Option<(AtomicOrdering, SyncScope)>,
    ) -> BcResult<MemFlags> {
        let mut mem = ir(self.at, MemFlags::aligned(align))?;
        if volatile {
            mem = mem.with_volatile();
        }
        if let Some((ordering, scope)) = atomic {
            mem = mem.with_atomic(ordering, scope);
        }
        Ok(mem)
    }

    fn load(&mut self, o: &mut Ops<'_>, atomic: bool) -> BcResult<(TypeId, Value)> {
        let ptr = self.pointer(o, "load")?;
        if o.left() != if atomic { 5 } else { 3 } {
            return self.invalid("a load record without its type");
        }
        let ty = self.ty(o.next()?)?;
        let align = self.alignment(o.next()?)?;
        let volatile = o.next()? != 0;
        let mut order = None;
        if atomic {
            let ordering = self.ordering(o.next()?)?;
            let scope = self.scope(o.next()?)?;
            if !ordering.valid_for_load() {
                return self.invalid("atomic load cannot use Release ordering");
            }
            order = Some((ordering, scope));
        }
        let types = &self.module.types;
        if !types.is_sized(ty) || ty == TypeId::TOKEN {
            return self.mistyped("loading unsized types is not allowed");
        }
        if atomic && !self.atomic_type(ty) {
            return self.mistyped("atomic load operand must have a power-of-two byte size");
        }
        let align = align.unwrap_or_else(|| types.align_of(ty));
        let mem = self.mem_flags(align, volatile, order)?;
        Ok((ty, ir(self.at, self.b.load(ty, ptr.value, mem))?))
    }

    fn store(&mut self, o: &mut Ops<'_>, atomic: bool) -> BcResult<(TypeId, Value)> {
        let ptr = self.pointer(o, "store")?;
        let value = self.vt(o)?;
        let align = self.alignment(o.next()?)?;
        let volatile = o.next()? != 0;
        let mut order = None;
        if atomic {
            let ordering = self.ordering(o.next()?)?;
            let scope = self.scope(o.next()?)?;
            if !ordering.valid_for_store() {
                return self.invalid("atomic store cannot use Acquire ordering");
            }
            order = Some((ordering, scope));
        }
        let ty = value.ty;
        let types = &self.module.types;
        if !types.is_sized(ty) || ty == TypeId::TOKEN {
            return self.mistyped("storing unsized types is not allowed");
        }
        if atomic && !self.atomic_type(ty) {
            return self.mistyped("atomic store operand must have a power-of-two byte size");
        }
        let align = align.unwrap_or_else(|| types.align_of(ty));
        let mem = self.mem_flags(align, volatile, order)?;
        ir(self.at, self.b.store(value.value, ptr.value, mem))?;
        Ok((TypeId::VOID, Value::NONE))
    }

    fn alloca(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let ty = self.ty(o.next()?)?;
        let count_ty = self.ty(o.next()?)?;
        let count_id = o.next()?;
        let packed = o.next()?;
        let space = if o.left() > 0 {
            o.u32()?
        } else {
            self.module.types.data_layout().alloca_addr_space
        };
        let count = self.get(count_id, Some(count_ty))?;
        if packed & (1 << 6) == 0 {
            return fail(
                BcErrorKind::Unsupported("record"),
                self.at,
                "an alloca without its explicit type, of a bitcode before LLVM 19",
            );
        }
        let align = self.alignment((packed & 0x1f) | (((packed >> 8) & 0x7) << 5))?;
        let (in_alloca, swift_error) = (packed & (1 << 5) != 0, packed & (1 << 7) != 0);
        if !self.module.types.is_int(count_ty) {
            return self.mistyped("element count must have integer type");
        }
        let types = &self.module.types;
        if !types.is_sized(ty) || ty == TypeId::TOKEN {
            return self.mistyped("invalid type for alloca");
        }
        let result = if space == 0 { TypeId::PTR } else { ir(self.at, self.module.types.ptr(space))? };
        if in_alloca {
            self.note("alloca:inalloca");
            return Ok((result, self.stand_in(result)?));
        }
        if swift_error {
            self.note("alloca:swifterror");
            return Ok((result, self.stand_in(result)?));
        }
        let types = &self.module.types;
        if types.is_scalable(ty) {
            self.note("alloca:scalable");
            return Ok((result, self.stand_in(result)?));
        }
        let elem_size = types.alloc_size_of(ty);
        let align = align.unwrap_or_else(|| types.pref_align_of(ty));
        let literal = count.value.as_const().and_then(|id| self.module.consts.int_value(id));
        // A static alloca is a slot: in the entry block, with a count
        // that is a literal.
        if self.begun == 1
            && space == 0
            && let Some(size) = literal.and_then(|count| elem_size.checked_mul(count))
        {
            let slot = ir(self.at, self.b.add_slot(size, align))?;
            return Ok((result, ir(self.at, self.b.slot(slot))?));
        }
        Ok((result, ir(self.at, self.b.alloca(result, count.value, elem_size, align))?))
    }

    fn cmpxchg(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let ptr = self.pointer(o, "cmpxchg")?;
        let expected = self.vt(o)?;
        let ty = expected.ty;
        let replacement = self.v(o, ty)?;
        let volatile = o.next()? != 0;
        let success = self.ordering(o.next()?)?;
        let scope = self.scope(o.next()?)?;
        let failure = self.ordering(o.next()?)?;
        let weak = o.next()? != 0;
        let align = if o.left() > 0 { self.alignment(o.next()?)? } else { None };
        if !success.valid_for_rmw() {
            return self.invalid("invalid cmpxchg success ordering");
        }
        if !failure.valid_for_cmpxchg_failure() {
            return self.invalid("invalid cmpxchg failure ordering");
        }
        let types = &self.module.types;
        if !(types.is_int(ty) || types.is_ptr(ty)) || !self.atomic_type(ty) {
            return self.mistyped(
                "cmpxchg operand must be an integer or pointer of a power-of-two byte size",
            );
        }
        let align = align.unwrap_or_else(|| types.size_of(ty).next_power_of_two());
        let mem = self.mem_flags(align, volatile, Some((success, scope)))?;
        let result = ir(self.at, self.module.types.struct_type(&[ty, TypeId::I1], false))?;
        let value = self.b.cmpxchg(
            result,
            ptr.value,
            expected.value,
            replacement.value,
            mem,
            failure,
            weak,
        );
        Ok((result, ir(self.at, value)?))
    }

    fn atomic_rmw(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let ptr = self.pointer(o, "atomicrmw")?;
        let value = self.vt(o)?;
        let Some(&op) = usize::try_from(o.next()?).ok().and_then(|op| RmwOp::ALL.get(op)) else {
            return self.invalid("an atomicrmw operation out of range");
        };
        let volatile = o.next()? != 0;
        let ordering = self.ordering(o.next()?)?;
        let scope = self.scope(o.next()?)?;
        let align = if o.left() > 0 { self.alignment(o.next()?)? } else { None };
        if !ordering.valid_for_rmw() {
            return self.invalid("atomicrmw cannot be unordered");
        }
        let ty = value.ty;
        let types = &self.module.types;
        let (scalar, shape) = self.shape(ty);
        let class = match op {
            RmwOp::Xchg => types.is_int(ty) || types.is_float(ty) || types.is_ptr(ty),
            op if op.is_float() => {
                types.is_float(scalar) && !shape.is_some_and(|(_, scalable)| scalable)
            }
            _ => types.is_int(ty),
        };
        if !class || !self.atomic_type(ty) {
            return self.mistyped("atomicrmw operand has a type the operation does not take");
        }
        let align = align.unwrap_or_else(|| types.size_of(ty).next_power_of_two());
        let mem = self.mem_flags(align, volatile, Some((ordering, scope)))?;
        Ok((ty, ir(self.at, self.b.atomic_rmw(op, ty, ptr.value, value.value, mem))?))
    }

    fn fence(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let ordering = self.ordering(o.next()?)?;
        let scope = self.scope(o.next()?)?;
        if !ordering.valid_for_fence() {
            return self.invalid("fence cannot be unordered or monotonic");
        }
        ir(self.at, self.b.fence(ordering, scope))?;
        Ok((TypeId::VOID, Value::NONE))
    }

    // ---- addresses and aggregates ----

    fn index_value(&self, id: ConstId) -> Option<i64> {
        let module = &*self.module;
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

    /// `getelementptr`: a `PtrAdd` with every constant index folded into
    /// the offset, as `hod_ll` builds it.
    fn get_element_ptr(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let _flags = o.next()?;
        let source = self.ty(o.next()?)?;
        let base = self.vt(o)?;
        let (pointer, mut shape) = self.shape(base.ty);
        if !self.module.types.is_ptr(pointer) {
            return self.mistyped("base of getelementptr must be a pointer");
        }
        if !self.module.types.is_sized(source) {
            return self.mistyped("base element of getelementptr must be sized");
        }
        let mut indices = std::mem::take(&mut self.s.indices);
        indices.clear();
        let result = self.gep_indices(o, source, &mut shape, &mut indices);
        let built = result.and_then(|(offset, unsupported)| {
            let result = self.shaped(pointer, shape)?;
            if let Some(reason) = unsupported {
                self.note(reason);
                return Ok((result, self.stand_in(result)?));
            }
            Ok((result, ir(self.at, self.b.ptr_add(result, base.value, offset, &indices))?))
        });
        self.s.indices = indices;
        built
    }

    fn gep_indices(
        &mut self,
        o: &mut Ops<'_>,
        source: TypeId,
        shape: &mut Shape,
        indices: &mut Vec<(Value, u64)>,
    ) -> BcResult<(i64, Option<&'static str>)> {
        let mut offset = 0i64;
        let mut current = source;
        let mut first = true;
        let mut unsupported = None;
        while o.left() > 0 {
            let index = self.vt(o)?;
            let (index_scalar, index_shape) = self.shape(index.ty);
            if self.module.types.int_bits(index_scalar).is_none() {
                return self.mistyped("getelementptr index must be an integer");
            }
            if index_shape.is_some() {
                if shape.is_some() && *shape != index_shape {
                    return self
                        .mistyped("getelementptr vector index has a wrong number of elements");
                }
                *shape = index_shape;
            }
            let constant = index.value.as_const().and_then(|id| self.index_value(id));
            let types = &self.module.types;
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
                            return self.mistyped("invalid getelementptr indices");
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
                    _ => return self.mistyped("invalid getelementptr indices"),
                }
            };
            let Some(stride) = stride else {
                unsupported = Some("getelementptr:scalable");
                continue;
            };
            match constant {
                Some(index) => offset = offset.wrapping_add(index.wrapping_mul(stride as i64)),
                None => indices.push((index.value, stride)),
            }
        }
        Ok((offset, unsupported))
    }

    /// The rest of the record as an index path into `s.path`.
    fn index_path(&mut self, o: &mut Ops<'_>) -> BcResult<()> {
        self.s.path.clear();
        if o.left() == 0 {
            return self.invalid("an aggregate instruction without its indices");
        }
        while o.left() > 0 {
            let index = o.u32()?;
            self.s.path.push(index);
        }
        Ok(())
    }

    fn field_type(&self, agg: TypeId) -> Option<TypeId> {
        let types = &self.module.types;
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

    fn extract_value(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let agg = self.vt(o)?;
        self.index_path(o)?;
        let Some(field) = self.field_type(agg.ty) else {
            return self.mistyped("invalid indices for extractvalue");
        };
        if let Ok((offset, _)) = aggregate_field(&self.module.types, agg.ty, &self.s.path) {
            return Ok((field, ir(self.at, self.b.extract_value(field, agg.value, offset))?));
        }
        self.note("aggregate:no-fixed-offset");
        Ok((field, self.stand_in(field)?))
    }

    fn insert_value(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let agg = self.vt(o)?;
        let value = self.vt(o)?;
        self.index_path(o)?;
        let Some(field) = self.field_type(agg.ty) else {
            return self.mistyped("invalid indices for insertvalue");
        };
        if field != value.ty {
            return self.mistyped("insertvalue operand and field disagree in type");
        }
        match aggregate_field(&self.module.types, agg.ty, &self.s.path) {
            Ok((offset, _)) if u32::try_from(offset).is_ok() => Ok((
                agg.ty,
                ir(self.at, self.b.insert_value(agg.ty, agg.value, value.value, offset))?,
            )),
            Ok(_) => {
                self.note("insertvalue:offset-over-4GiB");
                Ok((agg.ty, self.stand_in(agg.ty)?))
            }
            Err(_) => {
                self.note("aggregate:no-fixed-offset");
                Ok((agg.ty, self.stand_in(agg.ty)?))
            }
        }
    }

    fn extract_element(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let vector = self.vt(o)?;
        let index = self.vt(o)?;
        let (elem, shape) = self.shape(vector.ty);
        if shape.is_none() || !self.module.types.is_int(index.ty) {
            return self.mistyped("invalid extractelement operands");
        }
        Ok((elem, ir(self.at, self.b.extract_element(elem, vector.value, index.value))?))
    }

    fn insert_element(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let vector = self.vt(o)?;
        let (elem, shape) = self.shape(vector.ty);
        if shape.is_none() {
            return self.mistyped("invalid insertelement operands");
        }
        let value = self.v(o, elem)?;
        let index = self.vt(o)?;
        if !self.module.types.is_int(index.ty) {
            return self.mistyped("invalid insertelement operands");
        }
        let ty = vector.ty;
        Ok((ty, ir(self.at, self.b.insert_element(ty, vector.value, value.value, index.value))?))
    }

    fn shuffle_vector(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let first = self.vt(o)?;
        let second = self.v(o, first.ty)?;
        let mask = self.vt(o)?;
        let invalid = "invalid shufflevector operands";
        let (elem, Some((len, scalable))) = self.shape(first.ty) else {
            return self.mistyped(invalid);
        };
        let Some(mask_id) = mask.value.as_const() else {
            return self.mistyped(invalid);
        };
        let module = &*self.module;
        let (mask_len, mask_scalable) = match module.types.kind(mask.ty) {
            TypeKind::Vector { elem: TypeId::I32, len } => (len, false),
            TypeKind::ScalableVector { elem: TypeId::I32, min_len } => (min_len, true),
            _ => return self.mistyped(invalid),
        };
        if mask_scalable != scalable {
            return self.mistyped(invalid);
        }
        let entry = |id: ConstId| match module.consts.get(id) {
            Const::Int { value, .. } => u32::try_from(value).ok().filter(|&m| m != SHUFFLE_UNDEF),
            Const::Zero(_) => Some(0),
            Const::Undef(_) | Const::Poison(_) => Some(SHUFFLE_UNDEF),
            _ => None,
        };
        let out = &mut self.s.mask;
        out.clear();
        let complete = match module.consts.get(mask_id) {
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
            return self.mistyped(invalid);
        }
        let result = self.shaped(elem, Some((mask_len, scalable)))?;
        let value =
            self.b.shuffle_vector(result, first.value, second.value, &self.s.mask);
        Ok((result, ir(self.at, value)?))
    }

    // ---- exceptions ----

    fn landing_pad(&mut self, o: &mut Ops<'_>) -> BcResult<(TypeId, Value)> {
        let ty = self.ty(o.next()?)?;
        let cleanup = o.next()? != 0;
        let count = o.next()?;
        self.s.clauses.clear();
        for _ in 0..count {
            let catch = match o.next()? {
                0 => true,
                1 => false,
                _ => return self.invalid("a landing-pad clause of no kind"),
            };
            let clause = self.vt(o)?;
            // A symbol's address is the constant it is.
            let constant = match (clause.value.as_const(), clause.value.as_sym()) {
                (Some(constant), _) => constant,
                (None, Some(sym)) => ir(self.at, self.module.consts.addr(sym, 0))?,
                (None, None) => {
                    return self.mistyped("a landing-pad clause that is not a constant");
                }
            };
            self.push_clause(catch, constant)?;
        }
        if !cleanup && self.s.clauses.is_empty() {
            return self.invalid("landingpad needs at least one clause or to be a cleanup");
        }
        Ok((ty, ir(self.at, self.b.landing_pad(ty, cleanup, &self.s.clauses))?))
    }

    fn push_clause(&mut self, catch: bool, constant: ConstId) -> BcResult<()> {
        let module = &*self.module;
        let clause_ty = module.consts.ty(constant);
        let array = matches!(module.types.kind(clause_ty), TypeKind::Array { .. });
        let clause = match (catch, array) {
            (true, false) if module.types.is_ptr(clause_ty) => Clause::Catch(constant),
            (false, true) => Clause::Filter(constant),
            (true, _) => return self.mistyped("'catch' clause has an invalid type"),
            (false, _) => return self.mistyped("'filter' clause has an invalid type"),
        };
        self.s.clauses.push(clause);
        Ok(())
    }

    // ---- terminators ----

    fn ret(&mut self, o: &mut Ops<'_>) -> BcResult<Done> {
        let value = if o.left() == 0 {
            None
        } else {
            let value = self.vt(o)?;
            if o.left() > 0 {
                return self.invalid("a ret of more than one value");
            }
            Some(value)
        };
        if value.map_or(TypeId::VOID, |value| value.ty) != self.ret_ty {
            return self.mistyped("value doesn't match function result type");
        }
        ir(self.at, self.b.ret(value.map(|value| value.value)))?;
        Ok((TypeId::VOID, Value::NONE, true))
    }

    fn br(&mut self, o: &mut Ops<'_>) -> BcResult<Done> {
        let target = self.block(o.next()?)?;
        if o.left() == 0 {
            ir(self.at, self.b.br(target))?;
            return Ok((TypeId::VOID, Value::NONE, true));
        }
        let if_false = self.block(o.next()?)?;
        let cond = self.v(o, TypeId::I1)?;
        ir(self.at, self.b.cond_br(cond.value, target, if_false))?;
        Ok((TypeId::VOID, Value::NONE, true))
    }

    fn switch(&mut self, o: &mut Ops<'_>) -> BcResult<Done> {
        let ty_id = o.next()?;
        if ty_id >> 16 == 0x4B5 {
            return fail(
                BcErrorKind::Unsupported("record"),
                self.at,
                "a switch in the format of LLVM 3.x",
            );
        }
        let ty = self.ty(ty_id)?;
        let cond = self.v(o, ty)?;
        let default = self.block(o.next()?)?;
        if !self.module.types.is_int(ty) {
            return self.mistyped("switch condition must have integer type");
        }
        let mut cases = std::mem::take(&mut self.s.cases);
        cases.clear();
        self.s.case_ids.clear();
        let mut result = Ok(());
        while o.left() > 0 {
            let case = o.next().and_then(|id| self.get(id, Some(ty)));
            let target = case.and_then(|case| Ok((case, self.block(o.next()?)?)));
            let (case, target) = match target {
                Ok(found) => found,
                Err(error) => {
                    result = Err(error);
                    break;
                }
            };
            let constant = case.value.as_const().filter(|&id| {
                matches!(self.module.consts.get(id), Const::Int { .. } | Const::WideInt { .. })
            });
            let Some(constant) = constant else {
                result = self.mistyped("case value is not a constant integer");
                break;
            };
            cases.push((constant, target));
            self.s.case_ids.push(constant.raw());
        }
        self.s.case_ids.sort_unstable();
        if result.is_ok() && self.s.case_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            result = self.invalid("duplicate case value in switch");
        }
        let built = result.and_then(|()| ir(self.at, self.b.switch(cond.value, default, &cases)));
        self.s.cases = cases;
        built?;
        Ok((TypeId::VOID, Value::NONE, true))
    }

    fn indirect_br(&mut self, o: &mut Ops<'_>) -> BcResult<Done> {
        let ty = self.ty(o.next()?)?;
        let address = self.v(o, ty)?;
        if !self.module.types.is_ptr(ty) {
            return self.mistyped("indirectbr operand must be a pointer");
        }
        self.s.targets.clear();
        while o.left() > 0 {
            let target = self.block(o.next()?)?;
            self.s.targets.push(target);
        }
        ir(self.at, self.b.indirect_br(address.value, &self.s.targets))?;
        Ok((TypeId::VOID, Value::NONE, true))
    }

    // ---- calls ----

    fn callee_info(&mut self, sym: SymId) -> CalleeInfo {
        let at = sym.index();
        if self.s.callees.len() <= at {
            self.s.callees.resize(at + 1, CalleeInfo::Unknown);
        }
        if let CalleeInfo::Unknown = self.s.callees[at] {
            let module = &*self.module;
            let symbol = module.symbol(sym);
            let info = match std::str::from_utf8(symbol.name()) {
                Ok(name) if name.starts_with("llvm.") && symbol.is_function() => {
                    if let Some(rule) = no_code_intrinsic(name) {
                        CalleeInfo::NoCode(rule)
                    } else {
                        let found = resolve_intrinsic(&module.types, name, symbol.ty());
                        let known = match found.intrinsic {
                            Intrinsic::Other(_) => None,
                            intrinsic => Intrinsic::ALL
                                .iter()
                                .copied()
                                .find(|member| member.name() == intrinsic.name()),
                        };
                        if let Some(member) = known {
                            let mut overloads = [TypeId::VOID; 3];
                            let given = found.overloads();
                            overloads[..given.len()].copy_from_slice(given);
                            CalleeInfo::Known(member, overloads, given.len() as u8)
                        } else {
                            CalleeInfo::Other
                        }
                    }
                }
                _ => CalleeInfo::Plain,
            };
            self.s.callees[at] = info;
        }
        self.s.callees[at]
    }

    /// The callee operand.
    fn callee(&mut self, o: &mut Ops<'_>) -> BcResult<(Target, TypeId)> {
        let rel = o.next()?;
        let id = u64::from((self.next as u32).wrapping_sub(rel as u32));
        if id < self.next {
            if self.local_asm(id).is_some() {
                return Ok((Target::Asm(id), TypeId::PTR));
            }
            if let Some(global) = self.values.global(id)
                && let Some(sym) = global.sym
            {
                if global.ptr_ty == TypeId::PTR {
                    return Ok((Target::Sym(sym), TypeId::PTR));
                }
                return Ok((Target::Value(ir(self.at, Value::sym(sym))?), global.ptr_ty));
            }
            let local = self.get(id, None)?;
            if let Some(sym) = local.value.as_sym()
                && local.ty == TypeId::PTR
            {
                return Ok((Target::Sym(sym), local.ty));
            }
            return Ok((Target::Value(local.value), local.ty));
        }
        let ty = self.ty(o.next()?)?;
        let local = self.get(id, Some(ty))?;
        Ok((Target::Value(local.value), ty))
    }

    /// The attributes of a call site: the kept function flags (noting
    /// the names the groups reject), the return attributes, and the
    /// arguments' into `s.arg_attrs` (one per argument).
    fn call_attrs(
        &mut self,
        list: u64,
        args: usize,
        fn_only: bool,
    ) -> BcResult<(u32, ParamAttrs, bool)> {
        self.s.arg_attrs.clear();
        self.s.arg_attrs.resize(args, ParamAttrs::NONE);
        let (mut flags, mut ret_attrs, mut any) = (0, ParamAttrs::NONE, false);
        if list == 0 {
            return Ok((flags, ret_attrs, any));
        }
        let Some(groups) = self.bits.attr_list(list) else {
            return self.invalid("an attribute list that is not defined");
        };
        let mut fn_groups = Vec::new();
        let mut scratch = Vec::new();
        for &group in groups {
            let (index, rec) = self.group(group)?;
            if index == FUNCTION_INDEX {
                fn_groups.push((group, rec));
                continue;
            }
            if fn_only {
                continue;
            }
            attrs::decode(&rec.ops(&self.bits.raw.pool)[2..], rec.pos, &mut scratch)?;
            let types = &self.bits.types;
            let mut notes = 0u8;
            let mut type_of = |id: u64| {
                notes |= types.notes_of(id);
                types.get(id, rec.pos)
            };
            let mut pending = Vec::new();
            let built = attrs::param_attrs(&scratch, rec.pos, &mut type_of, &mut |name| {
                pending.push(name);
            });
            self.note_bits(notes);
            for name in pending {
                self.note(name);
            }
            let built = built?;
            if index == 0 {
                ret_attrs = built;
            } else {
                let slot = usize::try_from(index - 1).ok().and_then(|i| self.s.arg_attrs.get_mut(i));
                let Some(slot) = slot else {
                    return self.invalid("attributes of an argument the call does not have");
                };
                *slot = built;
                any |= built != ParamAttrs::NONE;
            }
        }
        for (group, rec) in fn_groups {
            let at = self.call_group(group, rec)?;
            let (_, kept, ref rejected) = self.s.fn_groups[at];
            flags |= kept;
            let names: Vec<String> = rejected.iter().map(|name| format!("attr:{name}")).collect();
            for name in names {
                self.note(name);
            }
        }
        Ok((flags & (FnAttrs::NORETURN | FnAttrs::NOUNWIND | FnAttrs::RETURNS_TWICE), ret_attrs, any))
    }

    /// The end of a call that is not emitted: an `invoke` or `callbr`
    /// ends the block with a stand-in branch.
    fn stand_in_call(&mut self, kind: Kind, ret: TypeId, normal: BlockId) -> BcResult<Done> {
        let value = self.stand_in(ret)?;
        if kind == Kind::Call {
            return Ok((ret, value, false));
        }
        ir(self.at, self.b.br(normal))?;
        Ok((ret, value, true))
    }

    /// `CALL`, `INVOKE` or `CALLBR`.
    #[allow(clippy::too_many_lines)] // One record, read in one place.
    fn call(&mut self, o: &mut Ops<'_>, kind: Kind) -> BcResult<Done> {
        let list = o.next()?;
        let cc_info = o.next()?;
        let mut normal = BlockId::new(0);
        let mut unwind = BlockId::new(0);
        self.s.targets.clear();
        let (cc, tail, explicit) = match kind {
            Kind::Call => {
                if cc_info & (1 << 17) != 0 {
                    o.next()?;
                }
                let tail = if cc_info & (1 << 14) != 0 {
                    TailKind::MustTail
                } else if cc_info & (1 << 16) != 0 {
                    TailKind::NoTail
                } else if cc_info & 1 != 0 {
                    TailKind::Tail
                } else {
                    TailKind::None
                };
                ((cc_info & 0x7ff) >> 1, tail, cc_info & (1 << 15) != 0)
            }
            Kind::Invoke => {
                normal = self.block(o.next()?)?;
                unwind = self.block(o.next()?)?;
                (cc_info & !(1 << 13), TailKind::None, cc_info & (1 << 13) != 0)
            }
            Kind::CallBr => {
                normal = self.block(o.next()?)?;
                let count = o.next()?;
                for _ in 0..count {
                    let target = self.block(o.next()?)?;
                    self.s.targets.push(target);
                }
                ((cc_info >> 1) & 0x3ff, TailKind::None, cc_info & (1 << 15) != 0)
            }
        };
        if !explicit {
            return fail(
                BcErrorKind::Unsupported("record"),
                self.at,
                "a call without its explicit function type, of a bitcode before LLVM 19",
            );
        }
        let cc = ir(self.at, CallConv::from_llvm(u32::try_from(cc).unwrap_or(u32::MAX)))?;
        let fn_ty = self.ty(o.next()?)?;
        let TypeKind::Function { ret, params, variadic } = self.module.types.kind(fn_ty) else {
            return self.mistyped("a call whose type is not a function type");
        };
        let params = params.to_vec();
        let (target, _) = self.callee(o)?;
        let info = match target {
            Target::Sym(sym) => self.callee_info(sym),
            _ => CalleeInfo::Plain,
        };
        let mut bundles = std::mem::take(&mut self.s.bundles);
        let result = self.call_rest(
            o,
            (kind, list),
            (cc, tail),
            fn_ty,
            (ret, &params, variadic),
            (target, info),
            (normal, unwind),
            &bundles,
        );
        bundles.clear();
        self.s.bundles = bundles;
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn call_rest(
        &mut self,
        o: &mut Ops<'_>,
        (kind, list): (Kind, u64),
        (cc, tail): (CallConv, TailKind),
        fn_ty: TypeId,
        (ret, params, variadic): (TypeId, &[TypeId], bool),
        (target, info): (Target, CalleeInfo),
        (normal, unwind): (BlockId, BlockId),
        bundles: &[(u32, u32, u32, u32)],
    ) -> BcResult<Done> {
        // A debug intrinsic is a debug record in the old form: its
        // arguments are not values.
        if let CalleeInfo::NoCode(NoCode::DebugRecord) = info {
            self.call_attrs(list, 0, true)?;
            if self.s.bundle_metadata && !bundles.is_empty() {
                self.note("bundle:metadata-input");
            }
            return self.stand_in_call(kind, ret, normal);
        }

        // The arguments.
        self.s.vals.clear();
        let mut metadata = false;
        for &param in params {
            match param {
                TypeId::LABEL => return self.mistyped("a block where a value is expected"),
                TypeId::METADATA => {
                    o.next()?;
                    metadata = true;
                    self.s.vals.push(Value::NONE);
                }
                _ => {
                    let arg = self.v(o, param)?;
                    self.s.vals.push(arg.value);
                }
            }
        }
        if variadic {
            while o.left() > 0 {
                let arg = self.vt(o)?;
                if arg.ty == TypeId::METADATA {
                    metadata = true;
                }
                self.s.vals.push(arg.value);
            }
        } else if o.left() > 0 {
            return self.invalid("too many arguments specified");
        }
        let args = self.s.vals.len();
        let (flags, ret_attrs, any_attrs) = self.call_attrs(list, args, false)?;
        if self.s.bundle_metadata && !bundles.is_empty() {
            self.note("bundle:metadata-input");
        }

        // An intrinsic must be called with the type it is declared with.
        let llvm =
            matches!(info, CalleeInfo::NoCode(_) | CalleeInfo::Known(..) | CalleeInfo::Other);
        if llvm
            && let Target::Sym(sym) = target
            && self.module.symbol(sym).ty() != fn_ty
        {
            return self.mistyped("intrinsic called with incompatible signature");
        }

        // `musttail` in a variadic function forwards the arguments: what
        // `llvm-dis` writes as `...`.
        if tail == TailKind::MustTail && self.variadic {
            self.note("call:forwarded-varargs");
            return self.stand_in_call(kind, ret, normal);
        }

        if let CalleeInfo::NoCode(rule) = info {
            if kind != Kind::Call {
                self.note("invoke:no-code-intrinsic");
                return self.stand_in_call(kind, ret, normal);
            }
            if rule == NoCode::FirstArg {
                if params.first() != Some(&ret) {
                    return self.mistyped(
                        "the first argument of this intrinsic has another type than its result",
                    );
                }
                return Ok((ret, self.s.vals[0], false));
            }
            return self.stand_in_call(kind, ret, normal);
        }

        if metadata {
            let construct = match target {
                Target::Sym(sym) => format!(
                    "metadata-arg:{}",
                    String::from_utf8_lossy(self.module.symbol(sym).name())
                ),
                _ => "metadata-arg:indirect".to_string(),
            };
            self.note(construct);
            return self.stand_in_call(kind, ret, normal);
        }

        let callee = match target {
            Target::Sym(sym) => match info {
                CalleeInfo::Known(..) | CalleeInfo::Other => None,
                _ => Some(ir(self.at, Value::sym(sym))?),
            },
            Target::Value(value) => Some(value),
            Target::Asm(id) => Some(self.asm(id, fn_ty)?),
        };

        let (tags, vals) = (&self.s.bundle_tags, &self.s.bundle_vals);
        let bundle_list: Vec<Bundle<'_>> = bundles
            .iter()
            .map(|&(t0, t1, v0, v1)| Bundle {
                tag: &tags[t0 as usize..t1 as usize],
                inputs: &vals[v0 as usize..v1 as usize],
            })
            .collect();
        let args = &self.s.vals;
        let overloads;
        let mut desc = match (callee, info, target) {
            (Some(value), _, _) => CallDesc::new(value, fn_ty, args),
            (None, CalleeInfo::Known(intrinsic, types, len), _) => {
                overloads = types;
                CallDesc::intrinsic(intrinsic, &overloads[..usize::from(len)], fn_ty, args)
            }
            (None, _, Target::Sym(sym)) => {
                let Ok(name) = std::str::from_utf8(self.module.symbol(sym).name()) else {
                    return self.invalid("an intrinsic whose name is not UTF-8");
                };
                CallDesc::intrinsic(Intrinsic::Other(name), &[], fn_ty, args)
            }
            (None, _, _) => return self.invalid("an intrinsic callee that is not a symbol"),
        };
        desc.cc = cc;
        desc.tail = tail;
        desc.noreturn = flags & FnAttrs::NORETURN != 0;
        desc.nounwind = flags & FnAttrs::NOUNWIND != 0;
        desc.returns_twice = flags & FnAttrs::RETURNS_TWICE != 0;
        desc.ret_attrs = ret_attrs;
        desc.arg_attrs = if any_attrs { &self.s.arg_attrs } else { &[] };
        desc.bundles = &bundle_list;
        let at = self.at;
        let value = match kind {
            Kind::Invoke => ir(at, self.b.invoke(ret, &desc, normal, unwind))?,
            Kind::CallBr => ir(at, self.b.callbr(ret, &desc, normal, &self.s.targets))?,
            Kind::Call => ir(at, self.b.call(ret, &desc))?,
        };
        Ok((ret, value, kind != Kind::Call))
    }

    /// The inline-assembly callee of function constant `id`:
    /// `[fnty, flags, asmlen, asm…, conlen, constraints…]`.
    fn asm(&mut self, id: u64, fn_ty: TypeId) -> BcResult<Value> {
        let Some(rec) = self.local_asm(id) else {
            return self.invalid("inline assembly that is not a constant");
        };
        if rec.code != crate::codes::cst::INLINEASM {
            return fail(
                BcErrorKind::Unsupported("record"),
                rec.pos,
                "inline assembly in the format of a bitcode before LLVM 19",
            );
        }
        let ops = rec.ops(&self.s.const_pool);
        let short = || fail(BcErrorKind::Invalid, rec.pos, "an inline-assembly record cut short");
        let [_, flags, rest @ ..] = ops else { return short() };
        let Some((&len, rest)) = rest.split_first() else { return short() };
        let len = usize::try_from(len).unwrap_or(usize::MAX);
        if rest.len() < len {
            return short();
        }
        let (template, rest) = rest.split_at(len);
        let Some((&len, rest)) = rest.split_first() else { return short() };
        let len = usize::try_from(len).unwrap_or(usize::MAX);
        if rest.len() != len {
            return short();
        }
        let template = attrs::chars(template, rec.pos)?;
        let constraints = attrs::chars(rest, rec.pos)?;
        let mut kept = 0u8;
        for (bit, flag) in [
            (1, InlineAsm::SIDE_EFFECT),
            (2, InlineAsm::ALIGN_STACK),
            (4, InlineAsm::INTEL_DIALECT),
            (8, InlineAsm::UNWIND),
        ] {
            if flags & bit != 0 {
                kept |= flag;
            }
        }
        let asm = InlineAsm { template: &template, constraints: &constraints, flags: kept, ty: fn_ty };
        let id = ir(self.at, self.module.inline_asm(&asm))?;
        ir(self.at, Value::asm(id))
    }
}
