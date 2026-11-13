//! The function builder (`spec/ir.md` §5.1, §5.3, §5.6): the only way a
//! [`Body`] comes into being.
//!
//! A producer creates blocks, begins them one at a time in layout order,
//! and appends instructions to the open block until it appends a
//! terminator. Every append states the result type; the builder does not
//! look types up, so it holds no borrow of the module and one module can
//! feed many builders on many threads.
//!
//! What the builder checks as it goes is what keeps the body's tables
//! sound: an open block exists, `phi`s come first, every operand names
//! something that exists in this body, counts fit their fields. Everything
//! that needs the type table (operand and result types, argument counts,
//! atomic orderings, `phi` lists against predecessors) is
//! [`Body::verify`]'s, which tests run on every body and a release build
//! runs on none. The split is by cost: the eager checks are a few compares
//! per operand.

use crate::attrs::{CallConv, ParamAttrs};
use crate::body::{
    BlockData, Body, Bundle, BundleData, CALL_ALWAYS_INLINE, CALL_CONVERGENT, CALL_NOINLINE,
    CALL_NORETURN, CALL_NOUNWIND, CALL_RETURNS_TWICE, CallData, Callee, Clause, DbgDeclare,
    DbgRecordData, DbgRecordKind, Fixup, Incoming, LifetimeMarker, MetaData, PadData, Place,
    Reserved, Slot, SlotVar, UNSET, UseFacts,
};
use crate::debug::expr::ExprId;
use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::{BlockId, ConstId, DiId, LocId, SymId, TypeId};
use crate::inst::{
    AtomicOrdering, FloatPredicate, InstData, IntPredicate, MemFlags, Opcode, PTRADD_POOLED, RmwOp,
    SyncScope, TailKind, align_log2, word,
};
use crate::intern::Layering;
use crate::intrinsic::Intrinsic;
use crate::module::Module;
use crate::types::TypeKind;
use crate::value::{
    InstId, MetaId, RESERVED_BASE, SlotId, TAG_ARG, TAG_ASM, TAG_CONST, TAG_INST, TAG_META,
    TAG_SLOT, TAG_SYM, Value,
};

/// Everything a call says besides its result type (§5.4, §5.7).
///
/// Start from [`CallDesc::new`] or [`CallDesc::intrinsic`] and set the
/// fields that differ from a plain C call.
// One field per kept call-site flag of LLVM's, as written in the IR.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug)]
pub struct CallDesc<'a> {
    /// What is called.
    pub callee: Callee<'a>,
    /// The overload types of a known intrinsic, in the order of its name's
    /// suffixes. Empty for anything else.
    pub overloads: &'a [TypeId],
    /// The function type the call is made with.
    pub fn_ty: TypeId,
    /// The arguments.
    pub args: &'a [Value],
    /// The calling convention.
    pub cc: CallConv,
    /// The tail-call marker.
    pub tail: TailKind,
    /// `noreturn` on the call.
    pub noreturn: bool,
    /// `nounwind` on the call.
    pub nounwind: bool,
    /// `returns_twice` on the call.
    pub returns_twice: bool,
    /// `convergent` on the call: kept, and constrains transformations
    /// only (`spec/ir.md` §5.7).
    pub convergent: bool,
    /// `alwaysinline` on the call (WP-188).
    pub always_inline: bool,
    /// `noinline` on the call (WP-188).
    pub noinline: bool,
    /// The kept attributes of the return value.
    pub ret_attrs: ParamAttrs,
    /// The kept attributes of each argument: empty when none has any,
    /// else one entry per argument.
    pub arg_attrs: &'a [ParamAttrs],
    /// The operand bundles.
    pub bundles: &'a [Bundle<'a>],
}

impl<'a> CallDesc<'a> {
    /// A C-convention call of `callee` with `args`, no attributes and no
    /// bundles.
    pub fn new(callee: Value, fn_ty: TypeId, args: &'a [Value]) -> CallDesc<'a> {
        CallDesc {
            callee: Callee::Value(callee),
            overloads: &[],
            fn_ty,
            args,
            cc: CallConv::C,
            tail: TailKind::None,
            noreturn: false,
            nounwind: false,
            returns_twice: false,
            convergent: false,
            always_inline: false,
            noinline: false,
            ret_attrs: ParamAttrs::NONE,
            arg_attrs: &[],
            bundles: &[],
        }
    }

    /// A call of `intrinsic` with these overload types (§5.5).
    pub fn intrinsic(
        intrinsic: Intrinsic<'a>,
        overloads: &'a [TypeId],
        fn_ty: TypeId,
        args: &'a [Value],
    ) -> CallDesc<'a> {
        CallDesc {
            callee: Callee::Intrinsic(intrinsic),
            overloads,
            ..CallDesc::new(Value::NONE, fn_ty, args)
        }
    }
}

/// Builds one function's [`Body`].
///
/// ```
/// use hod_ir::{FunctionBuilder, Module, Opcode, SymId, TargetSpec, TypeId};
/// let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
/// let fn_ty = m.types.function(TypeId::I32, &[TypeId::I32, TypeId::I32], false)?;
/// let mut b = FunctionBuilder::new(&m, SymId::new(0), fn_ty)?;
/// let entry = b.create_block()?;
/// b.begin_block(entry)?;
/// let sum = b.binary(Opcode::Add, TypeId::I32, b.arg(0)?, b.arg(1)?)?;
/// b.ret(Some(sum))?;
/// let body = b.finish()?;
/// body.verify(&m)?;
/// assert_eq!(body.inst_count(), 2);
/// assert!(body.uses(sum.as_inst().unwrap()).only_use_is_next());
/// # Ok::<(), hod_ir::IrError>(())
/// ```
#[derive(Debug)]
pub struct FunctionBuilder {
    body: Body,
    /// The open block, or `UNSET`.
    cur: u32,
    /// The open block's first instruction.
    cur_first: u32,
    /// Whether the open block has an instruction that is not a `phi`.
    past_phis: bool,
    /// The entry block, or `UNSET` before the first block is begun.
    entry: u32,
    /// The location given to instructions appended from now on.
    loc: LocId,
    with_locs: bool,
    /// Where the debug records appended from now on stand: before this
    /// instruction, or `UNSET` for the next one appended.
    record_at: u32,
}

#[cold]
fn bad<T>(kind: IrErrorKind, message: String) -> IrResult<T> {
    err(kind, message)
}

/// Fails with `TableFull` unless `more` entries can be added to a table of
/// `len` whose indices are `u32`.
#[inline]
fn room(len: usize, more: usize, what: &str) -> IrResult<()> {
    if len.checked_add(more).is_none_or(|total| total >= UNSET as usize) {
        return bad(IrErrorKind::TableFull, format!("no room for {more} more {what}"));
    }
    Ok(())
}

#[inline]
fn low(value: u64) -> Value {
    word(value as u32)
}

#[inline]
fn high(value: u64) -> Value {
    word((value >> 32) as u32)
}

impl FunctionBuilder {
    /// A builder for the body of function symbol `sym`, whose type is
    /// `fn_ty`.
    ///
    /// # Errors
    /// `InvalidType` if `fn_ty` is not a function type.
    pub fn new<L: Layering>(
        module: &Module<L>,
        sym: SymId,
        fn_ty: TypeId,
    ) -> IrResult<FunctionBuilder> {
        Self::reusing(module, sym, fn_ty, Body::empty())
    }

    /// [`FunctionBuilder::new`], building in the allocations of `storage`:
    /// a body that code generation is done with, or what
    /// [`FunctionBuilder::abandon`] returned. Once the tables have grown to
    /// the size of the largest function, building allocates nothing.
    ///
    /// # Errors
    /// `InvalidType` if `fn_ty` is not a function type.
    pub fn reusing<L: Layering>(
        module: &Module<L>,
        sym: SymId,
        fn_ty: TypeId,
        mut storage: Body,
    ) -> IrResult<FunctionBuilder> {
        let TypeKind::Function { params, .. } = module.types.kind(fn_ty) else {
            return bad(IrErrorKind::InvalidType, format!("{fn_ty:?} is not a function type"));
        };
        if params.len() > Value::MAX_INDEX as usize {
            return bad(IrErrorKind::TableFull, "too many parameters".to_string());
        }
        storage.clear();
        storage.sym = sym;
        storage.fn_ty = fn_ty;
        storage.arg_count = params.len() as u32;
        storage.arg_uses.resize(params.len(), UseFacts::NONE);
        Ok(FunctionBuilder {
            body: storage,
            cur: UNSET,
            cur_first: 0,
            past_phis: false,
            entry: UNSET,
            loc: LocId::NONE,
            with_locs: false,
            record_at: UNSET,
        })
    }

    /// Gives up on the function and returns the storage, empty, for
    /// [`FunctionBuilder::reusing`].
    pub fn abandon(mut self) -> Body {
        self.body.clear();
        self.body
    }

    // ---- blocks ----

    /// A new block, not yet placed. Block ids count up from 0 in creation
    /// order.
    ///
    /// # Errors
    /// `TableFull` after 2^32 - 1 blocks.
    pub fn create_block(&mut self) -> IrResult<BlockId> {
        room(self.body.blocks.len(), 1, "blocks")?;
        let id = BlockId::new(self.body.blocks.len() as u32);
        self.body.blocks.push(BlockData { first: UNSET, end: UNSET, phis: 0 });
        Ok(id)
    }

    /// Places `block` next in the layout and opens it: instructions are
    /// appended to it until a terminator. The first block begun is the
    /// entry block.
    ///
    /// # Errors
    /// `BadBlock` if a block is open or `block` was begun before;
    /// `InvalidOperand` if `block` is not a block of this builder.
    pub fn begin_block(&mut self, block: BlockId) -> IrResult<()> {
        self.check_block(block)?;
        if self.cur != UNSET {
            return bad(
                IrErrorKind::BadBlock,
                format!("{block:?} begun while bb{} has no terminator", self.cur),
            );
        }
        if self.body.blocks[block.index()].first != UNSET {
            return bad(IrErrorKind::BadBlock, format!("{block:?} begun twice"));
        }
        let first = self.body.insts.len() as u32;
        self.body.blocks[block.index()].first = first;
        self.body.layout.push(block);
        self.cur = block.raw();
        self.cur_first = first;
        self.past_phis = false;
        if self.entry == UNSET {
            self.entry = block.raw();
        }
        Ok(())
    }

    /// The open block.
    pub fn current_block(&self) -> Option<BlockId> {
        (self.cur != UNSET).then(|| BlockId::new(self.cur))
    }

    /// The number of blocks created so far.
    pub fn block_count(&self) -> usize {
        self.body.blocks.len()
    }

    /// The number of instructions appended so far.
    pub fn inst_count(&self) -> usize {
        self.body.insts.len()
    }

    // ---- values ----

    /// The number of arguments.
    pub fn arg_count(&self) -> u32 {
        self.body.arg_count
    }

    /// Argument `index`.
    ///
    /// # Errors
    /// `InvalidOperand` if the function has no such parameter.
    pub fn arg(&self, index: u32) -> IrResult<Value> {
        if index >= self.body.arg_count {
            return bad(
                IrErrorKind::InvalidOperand,
                format!("argument {index} of a function with {}", self.body.arg_count),
            );
        }
        Ok(Value::from_parts(TAG_ARG, index))
    }

    /// A value of type `ty` to be defined later (§5.3): usable as an
    /// operand at once, and to be given its definition with
    /// [`FunctionBuilder::resolve`] before [`FunctionBuilder::finish`].
    ///
    /// # Errors
    /// `TableFull` after 2^28 reservations.
    pub fn reserve(&mut self, ty: TypeId) -> IrResult<Value> {
        let index = self.body.reserved.len();
        if index >= RESERVED_BASE as usize {
            return bad(IrErrorKind::TableFull, "too many reserved values".to_string());
        }
        self.body.reserved.push(Reserved { ty, target: Value::NONE });
        Ok(Value::from_parts(TAG_INST, RESERVED_BASE + index as u32))
    }

    /// Whether `value` came from [`FunctionBuilder::reserve`].
    pub fn is_reserved(&self, value: Value) -> bool {
        self.reserved_index(value).is_some()
    }

    /// Defines the reserved value `reserved` as `value`: every operand
    /// that named `reserved`, before or after this call, names `value` in
    /// the finished body. `value` may be anything an operand can be,
    /// another reserved value included.
    ///
    /// # Errors
    /// `InvalidOperand` if `reserved` is not an unresolved reserved value
    /// of this builder, or `value` is not a valid operand.
    pub fn resolve(&mut self, reserved: Value, value: Value) -> IrResult<()> {
        let Some(index) = self.reserved_index(reserved) else {
            return bad(
                IrErrorKind::InvalidOperand,
                format!("{reserved:?} is not a reserved value"),
            );
        };
        if !self.body.reserved[index].target.is_none() {
            return bad(IrErrorKind::InvalidOperand, "a reserved value resolved twice".to_string());
        }
        self.check(value)?;
        self.body.reserved[index].target = value;
        Ok(())
    }

    /// The type of `value`: as [`Body::value_type`], and the type a
    /// reserved value was reserved with.
    pub fn value_type(&self, module: &Module, value: Value) -> TypeId {
        match self.reserved_index(value) {
            Some(index) => self.body.reserved[index].ty,
            None => self.body.value_type(module, value),
        }
    }

    #[inline]
    fn reserved_index(&self, value: Value) -> Option<usize> {
        if value.tag() != TAG_INST || value.payload() < RESERVED_BASE {
            return None;
        }
        let index = (value.payload() - RESERVED_BASE) as usize;
        (index < self.body.reserved.len()).then_some(index)
    }

    // ---- stack slots and locations ----

    /// A fixed stack object of `size` bytes aligned to `align` (§5.2).
    ///
    /// # Errors
    /// `InvalidOperand` unless `align` is a power of two up to 2^32;
    /// `TableFull` after 2^29 slots.
    pub fn add_slot(&mut self, size: u64, align: u64) -> IrResult<SlotId> {
        let log2 = align_log2(align)?;
        let index = self.body.slots.len();
        if index > Value::MAX_INDEX as usize {
            return bad(IrErrorKind::TableFull, "too many stack slots".to_string());
        }
        self.body.slots.push(Slot::new(size, log2));
        Ok(SlotId::new(index as u32))
    }

    /// The address of `slot`.
    ///
    /// # Errors
    /// `InvalidOperand` if `slot` is not a slot of this builder.
    pub fn slot(&self, slot: SlotId) -> IrResult<Value> {
        if slot.index() >= self.body.slots.len() {
            return bad(IrErrorKind::InvalidOperand, format!("{slot:?} does not exist"));
        }
        Ok(Value::from_parts(TAG_SLOT, slot.raw()))
    }
    /// A metadata operand (§5.3) for an argument of an intrinsic call: the
    /// node `!{!"s0", …}` of `strings` if `node`, else the string
    /// `!"s0"`, which takes exactly one string.
    ///
    /// # Errors
    /// `InvalidOperand` for a string that is not exactly one string,
    /// `TableFull`.
    pub fn metadata(&mut self, node: bool, strings: &[&str]) -> IrResult<Value> {
        if !node && strings.len() != 1 {
            return bad(
                IrErrorKind::InvalidOperand,
                format!("a metadata string is one string, not {}", strings.len()),
            );
        }
        let bytes = strings.iter().map(|text| text.len()).sum::<usize>();
        room(self.body.names.len(), bytes, "name bytes")?;
        room(self.body.words.len(), 2 * strings.len(), "operand words")?;
        room(self.body.metas.len(), 1, "metadata operands")?;
        let id = MetaId::new(self.body.metas.len() as u32);
        let value = Value::meta(id)?;
        let first = self.body.words.len() as u32;
        for text in strings {
            self.body.words.extend([self.body.names.len() as u32, text.len() as u32]);
            self.body.names.push_str(text);
        }
        self.body.metas.push(MetaData { node, first, count: strings.len() as u32 });
        Ok(value)
    }

    /// Marks `slot` as carrying `!coro.outside.frame` (§5.2): the front
    /// end promises that the object never lives across a coroutine's
    /// suspend.
    ///
    /// # Errors
    /// `InvalidOperand` if `slot` is not a slot of this builder.
    pub fn set_slot_outside_frame(&mut self, slot: SlotId) -> IrResult<()> {
        match self.body.slots.get_mut(slot.index()) {
            Some(data) => {
                data.set_outside_frame();
                Ok(())
            }
            None => bad(IrErrorKind::InvalidOperand, format!("{slot:?} does not exist")),
        }
    }

    /// A lifetime marker (§5.2) before the next instruction appended: a
    /// call of `llvm.lifetime.start` (`start`) or `llvm.lifetime.end` on
    /// `ptr`, which may be a reserved value. The marker is not a use of
    /// `ptr` and generates no code.
    ///
    /// # Errors
    /// `BadBlock` without an open block; `InvalidOperand` if `ptr` does
    /// not exist in this body.
    pub fn lifetime_marker(&mut self, start: bool, ptr: Value) -> IrResult<()> {
        let before = self.open()?;
        self.check(ptr)?;
        room(self.body.markers.len(), 1, "lifetime markers")?;
        self.body.markers.push(LifetimeMarker { before: InstId::new(before), ptr, start });
        Ok(())
    }

    /// Records that the source variable `var`, declared at `loc`, lives in
    /// `slot` for the whole function (§5.2):
    /// [`FunctionBuilder::dbg_declare`] of the slot's address with the
    /// empty expression.
    ///
    /// # Errors
    /// `InvalidOperand` if `slot` is not a slot of this builder.
    pub fn set_slot_var(&mut self, slot: SlotId, var: DiId, loc: LocId) -> IrResult<()> {
        let address = self.slot(slot)?;
        self.dbg_declare(address, var, ExprId::EMPTY, loc)
    }

    // ---- debug records (§6.6) ----

    /// `#dbg_declare(address, var, expr)` at `loc`, or a call of
    /// `llvm.dbg.declare`: the source variable `var` lives, for the whole
    /// function, where `expr` says starting from `address`.
    ///
    /// `address` is a slot's address (the usual case, which becomes a
    /// [`SlotVar`]), or an argument, an instruction result, a constant or
    /// a symbol (a [`DbgDeclare`]), or [`Value::NONE`] for LLVM's empty
    /// metadata. It may be a reserved value. The declaration does not use
    /// the value; it has no position and needs no open block.
    ///
    /// # Errors
    /// `InvalidOperand` if `address` is neither [`Value::NONE`] nor
    /// something that exists in this body.
    pub fn dbg_declare(
        &mut self,
        address: Value,
        var: DiId,
        expr: ExprId,
        loc: LocId,
    ) -> IrResult<()> {
        if !address.is_none() {
            self.check(address)?;
        }
        let body = &mut self.body;
        room(body.slot_vars.len() + body.declares.len(), 1, "declared variables")?;
        if !address.is_none() && address.tag() == TAG_SLOT {
            let slot = SlotId::new(address.payload());
            body.slot_vars.push(SlotVar { slot, var, loc, expr });
        } else {
            body.declares.push(DbgDeclare { address, var, expr, loc });
        }
        Ok(())
    }

    /// `#dbg_value(value, var, expr)` at `loc`, or a call of
    /// `llvm.dbg.value`, before the next instruction appended: from there
    /// on `var` has the value `expr` makes from `value`.
    ///
    /// `value` is anything an operand can be, a reserved value included,
    /// or [`Value::NONE`] for LLVM's empty metadata. The record does not
    /// use it.
    ///
    /// # Errors
    /// `BadBlock` without an open block; `InvalidOperand` if `value` is
    /// neither [`Value::NONE`] nor something that exists in this body.
    pub fn dbg_value(&mut self, value: Value, var: DiId, expr: ExprId, loc: LocId) -> IrResult<()> {
        self.dbg_record(DbgRecordKind::Value, &[value], false, var, expr, None, loc)
    }

    /// [`FunctionBuilder::dbg_value`] of a `DIArgList`: `expr` names the
    /// values by `DW_OP_LLVM_arg`.
    ///
    /// # Errors
    /// As [`FunctionBuilder::dbg_value`]; `InvalidOperand` for more than
    /// 65535 values.
    pub fn dbg_value_list(
        &mut self,
        values: &[Value],
        var: DiId,
        expr: ExprId,
        loc: LocId,
    ) -> IrResult<()> {
        self.dbg_record(DbgRecordKind::Value, values, true, var, expr, None, loc)
    }

    /// `#dbg_assign(value, var, expr, id, address, address_expr)` at
    /// `loc`, or a call of `llvm.dbg.assign`, before the next instruction
    /// appended. The `DIAssignID` is not kept (§6.9).
    ///
    /// # Errors
    /// As [`FunctionBuilder::dbg_value`], for `value` and `address`.
    pub fn dbg_assign(
        &mut self,
        value: Value,
        var: DiId,
        expr: ExprId,
        address: Value,
        address_expr: ExprId,
        loc: LocId,
    ) -> IrResult<()> {
        let address = Some((address, address_expr));
        self.dbg_record(DbgRecordKind::Assign, &[value], false, var, expr, address, loc)
    }

    /// `#dbg_label(label)` at `loc`, or a call of `llvm.dbg.label`,
    /// before the next instruction appended.
    ///
    /// # Errors
    /// `BadBlock` without an open block.
    pub fn dbg_label(&mut self, label: DiId, loc: LocId) -> IrResult<()> {
        self.dbg_record(DbgRecordKind::Label, &[], false, label, ExprId::EMPTY, None, loc)
    }

    /// Places the debug records appended from now on before instruction
    /// `before` ([`FunctionBuilder::inst_count`] when it was appended), not
    /// before the next one: for a producer that reads an instruction's
    /// records after the instruction (LLVM's bitcode writes them there).
    /// The block of that instruction need not be open; `None` goes back
    /// to placing them before the next instruction appended.
    ///
    /// # Errors
    /// `InvalidOperand` if `before` is past the next instruction.
    pub fn place_records_before(&mut self, before: Option<usize>) -> IrResult<()> {
        self.record_at = match before {
            None => UNSET,
            Some(at) if at <= self.body.insts.len() && at < RESERVED_BASE as usize => at as u32,
            Some(at) => {
                return bad(
                    IrErrorKind::InvalidOperand,
                    format!("instruction {at} is past the next one"),
                );
            }
        };
        Ok(())
    }

    #[allow(clippy::too_many_arguments, reason = "the fields of one record")]
    fn dbg_record(
        &mut self,
        kind: DbgRecordKind,
        values: &[Value],
        arg_list: bool,
        var: DiId,
        expr: ExprId,
        address: Option<(Value, ExprId)>,
        loc: LocId,
    ) -> IrResult<()> {
        let before = if self.record_at == UNSET { self.open()? } else { self.record_at };
        let Ok(count) = u16::try_from(values.len()) else {
            return bad(IrErrorKind::InvalidOperand, "a debug record has too many values".into());
        };
        for &value in values.iter().chain(address.iter().map(|(value, _)| value)) {
            if !value.is_none() {
                self.check(value)?;
            }
        }
        let body = &mut self.body;
        room(body.vals.len(), values.len() + 1, "operands")?;
        room(body.dbg_records.len(), 1, "debug records")?;
        let vals = body.vals.len() as u32;
        body.vals.extend_from_slice(values);
        let addr_expr = match address {
            Some((value, expr)) => {
                body.vals.push(value);
                expr
            }
            None => ExprId::EMPTY,
        };
        body.dbg_records.push(DbgRecordData {
            before,
            var,
            expr,
            loc,
            vals,
            addr_expr,
            count,
            kind,
            arg_list,
        });
        Ok(())
    }

    /// Sets the source location of the instructions appended from now on;
    /// [`LocId::NONE`] for none. A body whose builder never set a location
    /// has no location array (§6).
    pub fn set_loc(&mut self, loc: LocId) {
        self.loc = loc;
        if loc != LocId::NONE && !self.with_locs {
            self.with_locs = true;
            self.body.locs.resize(self.body.insts.len(), LocId::NONE);
        }
    }

    /// Sets the source location of the instructions appended since the
    /// builder held `first` of them ([`FunctionBuilder::inst_count`]
    /// then), and of no other: for a producer that learns an
    /// instruction's location after the instructions it became (LLVM's
    /// text writes `!dbg` last). The location of the instructions
    /// appended from now on is not changed.
    pub fn set_loc_since(&mut self, first: usize, loc: LocId) {
        if loc == LocId::NONE && !self.with_locs {
            return;
        }
        if !self.with_locs {
            self.with_locs = true;
            self.body.locs.resize(self.body.insts.len(), LocId::NONE);
        }
        let first = first.min(self.body.locs.len());
        self.body.locs[first..].fill(loc);
    }

    // ---- the checks and the bookkeeping every append shares ----

    /// The id the next instruction gets, if one can be appended.
    #[inline]
    fn open(&self) -> IrResult<u32> {
        if self.cur == UNSET {
            return bad(
                IrErrorKind::BadBlock,
                "an instruction needs an open block: begin one".to_string(),
            );
        }
        let next = self.body.insts.len();
        if next >= RESERVED_BASE as usize {
            return bad(IrErrorKind::TableFull, "too many instructions".to_string());
        }
        Ok(next as u32)
    }

    /// Whether `value` can be an operand: it names something that exists.
    #[inline]
    fn check(&self, value: Value) -> IrResult<()> {
        let index = value.payload();
        let ok = match value.tag() {
            TAG_INST => {
                (index as usize) < self.body.insts.len()
                    || (index >= RESERVED_BASE
                        && ((index - RESERVED_BASE) as usize) < self.body.reserved.len())
            }
            TAG_ARG => index < self.body.arg_count,
            TAG_SLOT => (index as usize) < self.body.slots.len(),
            TAG_CONST | TAG_SYM => true,
            _ => false,
        };
        if ok { Ok(()) } else { Self::bad_operand(value) }
    }

    #[cold]
    fn bad_operand(value: Value) -> IrResult<()> {
        let why = match value.tag() {
            TAG_INST => "names no instruction or reserved value of this body",
            TAG_ARG => "names no parameter",
            TAG_SLOT => "names no stack slot",
            TAG_ASM => "is inline assembly, which can only be called",
            TAG_META => "is metadata, which can only be an argument of an intrinsic call",
            _ => "is no value",
        };
        err(IrErrorKind::InvalidOperand, format!("operand {value:?} {why}"))
    }

    #[inline]
    fn check_block(&self, block: BlockId) -> IrResult<()> {
        if block.index() >= self.body.blocks.len() {
            return bad(IrErrorKind::InvalidOperand, format!("{block:?} was not created"));
        }
        Ok(())
    }

    /// Records the use of `value` by instruction `user` of the open block.
    #[inline]
    fn note_use(&mut self, value: Value, user: u32, place: Place) {
        let index = value.payload();
        match value.tag() {
            TAG_INST => {
                if index >= RESERVED_BASE {
                    self.body.fixups.push(Fixup { place, user, block: self.cur, phi: false });
                } else {
                    let outside = index < self.cur_first;
                    self.body.uses[index as usize].add(outside, !outside && index + 1 == user);
                }
            }
            TAG_ARG => self.body.arg_uses[index as usize].add(self.cur != self.entry, false),
            _ => {}
        }
    }

    /// Appends the record. The caller has called `open`.
    #[inline]
    fn push(&mut self, op: Opcode, sub: u8, flags: u16, ty: TypeId, ops: [Value; 3]) -> Value {
        let id = self.body.insts.len() as u32;
        self.body.insts.push(InstData { op, sub, flags, ty, ops });
        self.body.uses.push(UseFacts::NONE);
        if self.with_locs {
            self.body.locs.push(self.loc);
        }
        if op.is_terminator() {
            self.body.blocks[self.cur as usize].end = id + 1;
            self.cur = UNSET;
        } else if op != Opcode::Phi {
            self.past_phis = true;
        }
        Value::from_parts(TAG_INST, id)
    }

    /// An instruction whose first `values` operand fields are values and
    /// whose other fields are raw words.
    #[inline]
    fn simple(
        &mut self,
        op: Opcode,
        sub: u8,
        flags: u16,
        ty: TypeId,
        ops: [Value; 3],
        values: usize,
    ) -> IrResult<Value> {
        let user = self.open()?;
        for &value in &ops[..values] {
            self.check(value)?;
        }
        for (slot, &value) in ops[..values].iter().enumerate() {
            self.note_use(value, user, Place::Inline(slot as u8));
        }
        Ok(self.push(op, sub, flags, ty, ops))
    }

    // ---- arithmetic, comparisons, casts ----

    /// One of the eighteen two-operand arithmetic instructions.
    ///
    /// # Errors
    /// `InvalidOperand` if `op` is not one of them, or an operand does not
    /// exist; `BadBlock` without an open block. Those two, and `TableFull`
    /// for a body past 2^28 instructions, are the errors of every append
    /// method; the ones below list only what they add.
    #[inline]
    pub fn binary(&mut self, op: Opcode, ty: TypeId, lhs: Value, rhs: Value) -> IrResult<Value> {
        if !op.is_binary() {
            return bad(IrErrorKind::InvalidOperand, format!("{op:?} is not a binary operation"));
        }
        self.simple(op, 0, 0, ty, [lhs, rhs, word(0)], 2)
    }

    /// `fneg`.
    ///
    /// # Errors
    /// As every append method.
    pub fn fneg(&mut self, ty: TypeId, arg: Value) -> IrResult<Value> {
        self.simple(Opcode::FNeg, 0, 0, ty, [arg, word(0), word(0)], 1)
    }

    /// `freeze`.
    ///
    /// # Errors
    /// As every append method.
    pub fn freeze(&mut self, ty: TypeId, arg: Value) -> IrResult<Value> {
        self.simple(Opcode::Freeze, 0, 0, ty, [arg, word(0), word(0)], 1)
    }

    /// `icmp`. `ty` is `i1`, or a vector of `i1` for vector operands.
    ///
    /// # Errors
    /// As every append method.
    #[inline]
    pub fn icmp(
        &mut self,
        pred: IntPredicate,
        ty: TypeId,
        lhs: Value,
        rhs: Value,
    ) -> IrResult<Value> {
        self.simple(Opcode::ICmp, pred as u8, 0, ty, [lhs, rhs, word(0)], 2)
    }

    /// `fcmp`.
    ///
    /// # Errors
    /// As every append method.
    pub fn fcmp(
        &mut self,
        pred: FloatPredicate,
        ty: TypeId,
        lhs: Value,
        rhs: Value,
    ) -> IrResult<Value> {
        self.simple(Opcode::FCmp, pred as u8, 0, ty, [lhs, rhs, word(0)], 2)
    }

    /// One of the fourteen casts: `arg` converted to `ty`.
    ///
    /// # Errors
    /// `InvalidOperand` if `op` is not a cast.
    #[inline]
    pub fn cast(&mut self, op: Opcode, ty: TypeId, arg: Value) -> IrResult<Value> {
        if !op.is_cast() {
            return bad(IrErrorKind::InvalidOperand, format!("{op:?} is not a cast"));
        }
        self.simple(op, 0, 0, ty, [arg, word(0), word(0)], 1)
    }

    // ---- memory ----

    /// `load`: a `ty` read from `ptr`.
    ///
    /// # Errors
    /// As every append method.
    #[inline]
    pub fn load(&mut self, ty: TypeId, ptr: Value, mem: MemFlags) -> IrResult<Value> {
        self.simple(Opcode::Load, 0, mem.raw(), ty, [ptr, word(0), word(0)], 1)
    }

    /// `store`: `value` written to `ptr`.
    ///
    /// # Errors
    /// As every append method.
    #[inline]
    pub fn store(&mut self, value: Value, ptr: Value, mem: MemFlags) -> IrResult<Value> {
        self.simple(Opcode::Store, 0, mem.raw(), TypeId::VOID, [value, ptr, word(0)], 2)
    }

    /// A dynamic stack allocation of `count * elem_size` bytes aligned to
    /// `align` (§5.2). `ty` is the pointer type of the result.
    ///
    /// # Errors
    /// `InvalidOperand` unless `align` is a power of two up to 2^32.
    pub fn alloca(
        &mut self,
        ty: TypeId,
        count: Value,
        elem_size: u64,
        align: u64,
    ) -> IrResult<Value> {
        let log2 = align_log2(align)?;
        self.simple(Opcode::Alloca, log2, 0, ty, [count, low(elem_size), high(elem_size)], 1)
    }

    /// `fence`.
    ///
    /// # Errors
    /// As every append method.
    pub fn fence(&mut self, ordering: AtomicOrdering, scope: SyncScope) -> IrResult<Value> {
        let flags = u16::from(scope == SyncScope::SingleThread);
        self.simple(Opcode::Fence, ordering as u8, flags, TypeId::VOID, [word(0); 3], 0)
    }

    /// `cmpxchg`. `ty` is `{ T, i1 }` for operands of type `T`; `mem`
    /// carries the success ordering.
    ///
    /// # Errors
    /// As every append method.
    #[allow(clippy::too_many_arguments)]
    pub fn cmpxchg(
        &mut self,
        ty: TypeId,
        ptr: Value,
        expected: Value,
        replacement: Value,
        mem: MemFlags,
        failure: AtomicOrdering,
        weak: bool,
    ) -> IrResult<Value> {
        let flags = mem.pack_cmpxchg(failure, weak);
        self.simple(Opcode::CmpXchg, 0, flags, ty, [ptr, expected, replacement], 3)
    }

    /// `atomicrmw`: the old value at `ptr`, which becomes `op(old, value)`.
    ///
    /// # Errors
    /// As every append method.
    pub fn atomic_rmw(
        &mut self,
        op: RmwOp,
        ty: TypeId,
        ptr: Value,
        value: Value,
        mem: MemFlags,
    ) -> IrResult<Value> {
        self.simple(Opcode::AtomicRmw, op as u8, mem.raw(), ty, [ptr, value, word(0)], 2)
    }

    // ---- addresses and aggregates ----

    /// `base + offset + Σ index * scale` (§5.4). Each index is
    /// sign-extended or truncated to the pointer's index width before the
    /// multiplication.
    ///
    /// # Errors
    /// As every append method.
    #[inline]
    pub fn ptr_add(
        &mut self,
        ty: TypeId,
        base: Value,
        offset: i64,
        indices: &[(Value, u64)],
    ) -> IrResult<Value> {
        let offset = offset as u64;
        if indices.is_empty() {
            return self.simple(Opcode::PtrAdd, 0, 0, ty, [base, low(offset), high(offset)], 1);
        }
        let user = self.open()?;
        self.check(base)?;
        for &(index, _) in indices {
            self.check(index)?;
        }
        room(self.body.vals.len(), indices.len() + 1, "operands")?;
        room(self.body.words.len(), 2 * indices.len() + 2, "operand words")?;
        let vals = self.body.vals.len() as u32;
        let words = self.body.words.len() as u32;
        self.body.vals.push(base);
        self.note_use(base, user, Place::Val(vals));
        self.body.words.extend([offset as u32, (offset >> 32) as u32]);
        for &(index, scale) in indices {
            let at = self.body.vals.len() as u32;
            self.body.vals.push(index);
            self.note_use(index, user, Place::Val(at));
            self.body.words.extend([scale as u32, (scale >> 32) as u32]);
        }
        let count = indices.len() as u32 + 1;
        let ops = [word(vals), word(count), word(words)];
        Ok(self.push(Opcode::PtrAdd, 0, PTRADD_POOLED, ty, ops))
    }

    /// `extractvalue`: the field of type `ty` at byte `offset` of `agg`.
    /// [`crate::aggregate_field`] turns an LLVM index list into the
    /// offset and the type.
    ///
    /// # Errors
    /// As every append method.
    pub fn extract_value(&mut self, ty: TypeId, agg: Value, offset: u64) -> IrResult<Value> {
        self.simple(Opcode::ExtractValue, 0, 0, ty, [agg, low(offset), high(offset)], 1)
    }

    /// `insertvalue`: `agg` (of type `ty`) with the field at byte `offset`
    /// replaced by `value`.
    ///
    /// # Errors
    /// `TypeTooLarge` for an offset of 4 GiB or more.
    pub fn insert_value(
        &mut self,
        ty: TypeId,
        agg: Value,
        value: Value,
        offset: u64,
    ) -> IrResult<Value> {
        let Ok(offset) = u32::try_from(offset) else {
            return bad(
                IrErrorKind::TypeTooLarge,
                format!("insertvalue at byte offset {offset}: the limit is 2^32 - 1"),
            );
        };
        self.simple(Opcode::InsertValue, 0, 0, ty, [agg, value, word(offset)], 2)
    }

    /// `extractelement`.
    ///
    /// # Errors
    /// As every append method.
    pub fn extract_element(&mut self, ty: TypeId, vector: Value, index: Value) -> IrResult<Value> {
        self.simple(Opcode::ExtractElement, 0, 0, ty, [vector, index, word(0)], 2)
    }

    /// `insertelement`.
    ///
    /// # Errors
    /// As every append method.
    pub fn insert_element(
        &mut self,
        ty: TypeId,
        vector: Value,
        value: Value,
        index: Value,
    ) -> IrResult<Value> {
        self.simple(Opcode::InsertElement, 0, 0, ty, [vector, value, index], 3)
    }

    /// `shufflevector`. `mask` has one entry per element of the result:
    /// an index into the concatenation of `first` and `second`, or
    /// [`crate::SHUFFLE_UNDEF`].
    ///
    /// # Errors
    /// As every append method.
    pub fn shuffle_vector(
        &mut self,
        ty: TypeId,
        first: Value,
        second: Value,
        mask: &[u32],
    ) -> IrResult<Value> {
        room(self.body.words.len(), mask.len() + 1, "operand words")?;
        let user = self.open()?;
        self.check(first)?;
        self.check(second)?;
        let start = self.body.words.len() as u32;
        self.body.words.push(mask.len() as u32);
        self.body.words.extend_from_slice(mask);
        self.note_use(first, user, Place::Inline(0));
        self.note_use(second, user, Place::Inline(1));
        Ok(self.push(Opcode::ShuffleVector, 0, 0, ty, [first, second, word(start)]))
    }

    // ---- phi, select, va_arg ----

    /// A `phi` of type `ty`. More incoming values can be added with
    /// [`FunctionBuilder::add_incoming`] at any time before `finish`.
    ///
    /// # Errors
    /// `BadBlock` if the open block already has an instruction that is
    /// not a `phi`; `InvalidOperand` for an incoming block that was not
    /// created.
    pub fn phi(&mut self, ty: TypeId, incoming: &[(Value, BlockId)]) -> IrResult<Value> {
        let user = self.open()?;
        if self.past_phis {
            return bad(
                IrErrorKind::BadBlock,
                format!("a phi after another kind of instruction in bb{}", self.cur),
            );
        }
        for &(value, block) in incoming {
            self.check(value)?;
            self.check_block(block)?;
        }
        room(self.body.pending.len(), incoming.len(), "phi operands")?;
        let result = self.push(Opcode::Phi, 0, 0, ty, [word(0); 3]);
        self.body.blocks[self.cur as usize].phis += 1;
        for &(value, block) in incoming {
            self.push_incoming(user, value, block);
        }
        Ok(result)
    }

    /// Adds `value`, arriving from `block`, to `phi`. A predecessor that
    /// reaches the `phi`'s block by several edges is added once per edge.
    ///
    /// # Errors
    /// `InvalidOperand` if `phi` is not a `phi` of this builder, `value`
    /// does not exist, or `block` was not created.
    pub fn add_incoming(&mut self, phi: Value, value: Value, block: BlockId) -> IrResult<()> {
        let index = phi.payload() as usize;
        let is_phi = phi.tag() == TAG_INST
            && self.body.insts.get(index).is_some_and(|inst| inst.op == Opcode::Phi);
        if !is_phi {
            return bad(IrErrorKind::InvalidOperand, format!("{phi:?} is not a phi"));
        }
        self.check(value)?;
        self.check_block(block)?;
        room(self.body.pending.len(), 1, "phi operands")?;
        self.push_incoming(index as u32, value, block);
        Ok(())
    }

    /// Records an incoming value and its use, which is located at the end
    /// of `block` (§5.6).
    fn push_incoming(&mut self, phi: u32, value: Value, block: BlockId) {
        let at = self.body.pending.len() as u32;
        let index = value.payload();
        match value.tag() {
            TAG_INST => {
                if index >= RESERVED_BASE {
                    let block = block.raw();
                    self.body.fixups.push(Fixup {
                        place: Place::Incoming(at),
                        user: phi,
                        block,
                        phi: true,
                    });
                } else {
                    // A block not begun yet cannot hold a value that
                    // already exists.
                    let outside = !self.body.blocks[block.index()].contains(index);
                    self.body.uses[index as usize].add(outside, false);
                }
            }
            TAG_ARG => self.body.arg_uses[index as usize].add(block.raw() != self.entry, false),
            _ => {}
        }
        self.body.pending.push(Incoming { phi, value, block: block.raw() });
    }

    /// `select`.
    ///
    /// # Errors
    /// As every append method.
    pub fn select(
        &mut self,
        ty: TypeId,
        cond: Value,
        if_true: Value,
        if_false: Value,
    ) -> IrResult<Value> {
        self.simple(Opcode::Select, 0, 0, ty, [cond, if_true, if_false], 3)
    }

    /// `va_arg`: the next variable argument, of type `ty`, from the
    /// `va_list` at `list`.
    ///
    /// # Errors
    /// As every append method.
    pub fn va_arg(&mut self, ty: TypeId, list: Value) -> IrResult<Value> {
        self.simple(Opcode::VaArg, 0, 0, ty, [list, word(0), word(0)], 1)
    }

    // ---- calls ----

    /// What a call can get wrong without the type table.
    fn check_call(&self, desc: &CallDesc<'_>) -> IrResult<()> {
        match desc.callee {
            Callee::Value(callee) => {
                if callee.tag() != TAG_ASM {
                    self.check(callee)?;
                }
                if !desc.overloads.is_empty() {
                    return bad(
                        IrErrorKind::InvalidOperand,
                        "overload types on a call that is not an intrinsic's".to_string(),
                    );
                }
            }
            Callee::Intrinsic(intrinsic) => {
                if desc.overloads.len() != intrinsic.overload_count() {
                    return bad(
                        IrErrorKind::InvalidOperand,
                        format!(
                            "{} takes {} overload types, not {}",
                            intrinsic.name(),
                            intrinsic.overload_count(),
                            desc.overloads.len()
                        ),
                    );
                }
            }
        }
        let mut values = 1 + desc.args.len();
        for &arg in desc.args {
            let meta = arg.tag() == TAG_META && matches!(desc.callee, Callee::Intrinsic(_));
            if !(meta && (arg.payload() as usize) < self.body.metas.len()) {
                self.check(arg)?;
            }
        }
        for bundle in desc.bundles {
            values += bundle.inputs.len();
            for &input in bundle.inputs {
                self.check(input)?;
            }
        }
        if !desc.arg_attrs.is_empty() && desc.arg_attrs.len() != desc.args.len() {
            return bad(
                IrErrorKind::InvalidOperand,
                format!(
                    "{} argument attribute sets for {} arguments",
                    desc.arg_attrs.len(),
                    desc.args.len()
                ),
            );
        }
        if desc.bundles.len() > usize::from(u16::MAX) {
            return bad(IrErrorKind::TableFull, "too many operand bundles".to_string());
        }
        let name_bytes = desc.bundles.iter().map(|bundle| bundle.tag.len()).sum::<usize>()
            + match desc.callee {
                Callee::Intrinsic(Intrinsic::Other(name)) => name.len(),
                _ => 0,
            };
        room(self.body.vals.len(), values, "operands")?;
        room(self.body.names.len(), name_bytes, "name bytes")?;
        room(self.body.calls.len(), 1, "calls")?;
        room(self.body.bundles.len(), desc.bundles.len(), "operand bundles")?;
        room(self.body.param_attrs.len(), desc.args.len() + 1, "attribute sets")?;
        room(self.body.types.len(), desc.overloads.len(), "overload types")?;
        room(self.body.words.len(), 2, "operand words")
    }

    /// Appends a call that `check_call` passed.
    fn push_call(
        &mut self,
        op: Opcode,
        ty: TypeId,
        desc: &CallDesc<'_>,
        user: u32,
        normal: u32,
        unwind: u32,
    ) -> Value {
        let body = &mut self.body;
        let (callee, code, extra) = match desc.callee {
            Callee::Value(callee) => (callee, 0, 0),
            Callee::Intrinsic(intrinsic @ Intrinsic::Other(name)) => {
                let extra = body.words.len() as u32;
                body.words.extend([body.names.len() as u32, name.len() as u32]);
                body.names.push_str(name);
                (Value::NONE, intrinsic.code(), extra)
            }
            Callee::Intrinsic(intrinsic) => {
                let extra = body.types.len() as u32;
                body.types.extend_from_slice(desc.overloads);
                (Value::NONE, intrinsic.code(), extra)
            }
        };
        let start = body.vals.len() as u32;
        body.vals.push(callee);
        body.vals.extend_from_slice(desc.args);
        let first_bundle = body.bundles.len() as u32;
        for bundle in desc.bundles {
            body.bundles.push(BundleData {
                tag_start: body.names.len() as u32,
                tag_len: bundle.tag.len() as u32,
                first: body.vals.len() as u32,
                count: bundle.inputs.len() as u32,
            });
            body.names.push_str(bundle.tag);
            body.vals.extend_from_slice(bundle.inputs);
        }
        let count = body.vals.len() as u32 - start;
        let plain = desc.ret_attrs.is_none() && desc.arg_attrs.iter().all(|attrs| attrs.is_none());
        let attrs = if plain {
            UNSET
        } else {
            let attrs = body.param_attrs.len() as u32;
            body.param_attrs.push(desc.ret_attrs);
            body.param_attrs.extend_from_slice(desc.arg_attrs);
            body.param_attrs.resize(attrs as usize + 1 + desc.args.len(), ParamAttrs::NONE);
            attrs
        };
        let flags = desc.tail as u8
            | if desc.noreturn { CALL_NORETURN } else { 0 }
            | if desc.nounwind { CALL_NOUNWIND } else { 0 }
            | if desc.returns_twice { CALL_RETURNS_TWICE } else { 0 }
            | if desc.convergent { CALL_CONVERGENT } else { 0 }
            | if desc.always_inline { CALL_ALWAYS_INLINE } else { 0 }
            | if desc.noinline { CALL_NOINLINE } else { 0 };
        let index = body.calls.len() as u32;
        body.calls.push(CallData {
            inst: user,
            fn_ty: desc.fn_ty,
            nargs: desc.args.len() as u32,
            attrs,
            bundles: first_bundle,
            normal,
            unwind,
            extra,
            nbundles: desc.bundles.len() as u16,
            cc: desc.cc,
            intrinsic: code,
            noverloads: desc.overloads.len() as u8,
            flags,
        });
        for at in start..start + count {
            let value = self.body.vals[at as usize];
            self.note_use(value, user, Place::Val(at));
        }
        self.push(op, 0, 0, ty, [word(start), word(count), word(index)])
    }

    /// `call`. `ty` is the return type of `desc.fn_ty`.
    ///
    /// # Errors
    /// `InvalidOperand` for overload types that do not fit the callee, or
    /// argument attributes that are neither empty nor one per argument;
    /// `TableFull` past 65535 operand bundles.
    pub fn call(&mut self, ty: TypeId, desc: &CallDesc<'_>) -> IrResult<Value> {
        let user = self.open()?;
        self.check_call(desc)?;
        Ok(self.push_call(Opcode::Call, ty, desc, user, UNSET, UNSET))
    }

    /// `invoke`: a call that ends its block, continuing at `normal` when
    /// the callee returns and at `unwind` when it throws.
    ///
    /// # Errors
    /// As [`FunctionBuilder::call`]; `InvalidOperand` for a destination
    /// that was not created.
    pub fn invoke(
        &mut self,
        ty: TypeId,
        desc: &CallDesc<'_>,
        normal: BlockId,
        unwind: BlockId,
    ) -> IrResult<Value> {
        let user = self.open()?;
        self.check_call(desc)?;
        self.check_block(normal)?;
        self.check_block(unwind)?;
        Ok(self.push_call(Opcode::Invoke, ty, desc, user, normal.raw(), unwind.raw()))
    }

    /// `callbr`: a call (of inline assembly) that ends its block,
    /// continuing at `fallthrough` or at one of `indirect`.
    ///
    /// # Errors
    /// As [`FunctionBuilder::call`]; `InvalidOperand` for a destination
    /// that was not created.
    pub fn callbr(
        &mut self,
        ty: TypeId,
        desc: &CallDesc<'_>,
        fallthrough: BlockId,
        indirect: &[BlockId],
    ) -> IrResult<Value> {
        let user = self.open()?;
        self.check_call(desc)?;
        self.check_block(fallthrough)?;
        for &block in indirect {
            self.check_block(block)?;
        }
        room(self.body.words.len(), indirect.len() + 4, "operand words")?;
        let list = self.body.words.len() as u32;
        self.body.words.extend([indirect.len() as u32 + 1, fallthrough.raw()]);
        self.body.words.extend(indirect.iter().map(|block| block.raw()));
        Ok(self.push_call(Opcode::CallBr, ty, desc, user, fallthrough.raw(), list))
    }

    // ---- exceptions ----

    /// `landingpad`, of type `ty` (`{ ptr, i32 }` for the Itanium ABI).
    ///
    /// # Errors
    /// As every append method.
    pub fn landing_pad(
        &mut self,
        ty: TypeId,
        cleanup: bool,
        clauses: &[Clause],
    ) -> IrResult<Value> {
        let user = self.open()?;
        room(self.body.pads.len(), 1, "landing pads")?;
        room(self.body.clauses.len(), clauses.len(), "landing-pad clauses")?;
        let index = self.body.pads.len() as u32;
        self.body.pads.push(PadData {
            inst: user,
            clauses: self.body.clauses.len() as u32,
            nclauses: clauses.len() as u32,
            cleanup,
        });
        self.body.clauses.extend_from_slice(clauses);
        Ok(self.push(Opcode::LandingPad, 0, 0, ty, [word(index), word(0), word(0)]))
    }

    /// `resume`: continue unwinding with the exception `value`.
    ///
    /// # Errors
    /// As every append method.
    pub fn resume(&mut self, value: Value) -> IrResult<Value> {
        self.simple(Opcode::Resume, 0, 0, TypeId::VOID, [value, word(0), word(0)], 1)
    }

    /// A funclet instruction, with no operands (§5.4).
    ///
    /// # Errors
    /// `InvalidOperand` if `op` is not a funclet opcode.
    pub fn funclet(&mut self, op: Opcode, ty: TypeId) -> IrResult<Value> {
        if !op.is_funclet() {
            return bad(IrErrorKind::InvalidOperand, format!("{op:?} is not a funclet opcode"));
        }
        self.simple(op, 0, 0, ty, [word(0); 3], 0)
    }

    // ---- terminators ----

    /// `br`.
    ///
    /// # Errors
    /// `InvalidOperand` for a target that was not created.
    #[inline]
    pub fn br(&mut self, target: BlockId) -> IrResult<Value> {
        self.check_block(target)?;
        self.simple(Opcode::Br, 0, 0, TypeId::VOID, [word(target.raw()), word(0), word(0)], 0)
    }

    /// The two-way `br`.
    ///
    /// # Errors
    /// `InvalidOperand` for a target that was not created.
    #[inline]
    pub fn cond_br(&mut self, cond: Value, if_true: BlockId, if_false: BlockId) -> IrResult<Value> {
        self.check_block(if_true)?;
        self.check_block(if_false)?;
        let ops = [cond, word(if_true.raw()), word(if_false.raw())];
        self.simple(Opcode::CondBr, 0, 0, TypeId::VOID, ops, 1)
    }

    /// `switch`: each case is an integer constant of `cond`'s type and
    /// the block it selects.
    ///
    /// # Errors
    /// `InvalidOperand` for a target that was not created.
    pub fn switch(
        &mut self,
        cond: Value,
        default: BlockId,
        cases: &[(ConstId, BlockId)],
    ) -> IrResult<Value> {
        let user = self.open()?;
        self.check(cond)?;
        self.check_block(default)?;
        for &(_, block) in cases {
            self.check_block(block)?;
        }
        room(self.body.words.len(), 2 * cases.len() + 1, "operand words")?;
        let start = self.body.words.len() as u32;
        self.body.words.push(default.raw());
        self.body.words.extend(cases.iter().map(|&(_, block)| block.raw()));
        self.body.words.extend(cases.iter().map(|&(value, _)| value.raw()));
        self.note_use(cond, user, Place::Inline(0));
        let ops = [cond, word(start), word(cases.len() as u32)];
        Ok(self.push(Opcode::Switch, 0, 0, TypeId::VOID, ops))
    }

    /// `indirectbr`: a jump to the block whose address is `addr`, one of
    /// `targets`.
    ///
    /// # Errors
    /// `InvalidOperand` for a target that was not created.
    pub fn indirect_br(&mut self, addr: Value, targets: &[BlockId]) -> IrResult<Value> {
        let user = self.open()?;
        self.check(addr)?;
        for &block in targets {
            self.check_block(block)?;
        }
        room(self.body.words.len(), targets.len(), "operand words")?;
        let start = self.body.words.len() as u32;
        self.body.words.extend(targets.iter().map(|block| block.raw()));
        self.note_use(addr, user, Place::Inline(0));
        let ops = [addr, word(start), word(targets.len() as u32)];
        Ok(self.push(Opcode::IndirectBr, 0, 0, TypeId::VOID, ops))
    }

    /// `ret`; `None` for `ret void`.
    ///
    /// # Errors
    /// As every append method.
    #[inline]
    pub fn ret(&mut self, value: Option<Value>) -> IrResult<Value> {
        match value {
            Some(value) => {
                self.simple(Opcode::Ret, 0, 0, TypeId::VOID, [value, word(0), word(0)], 1)
            }
            None => {
                self.simple(Opcode::Ret, 0, 0, TypeId::VOID, [Value::NONE, word(0), word(0)], 0)
            }
        }
    }

    /// `unreachable`.
    ///
    /// # Errors
    /// As every append method.
    pub fn unreachable(&mut self) -> IrResult<Value> {
        self.simple(Opcode::Unreachable, 0, 0, TypeId::VOID, [word(0); 3], 0)
    }

    // ---- finish ----

    /// The finished body.
    ///
    /// # Errors
    /// `BadBlock` if no block was begun, a created block was never begun,
    /// or the last block has no terminator; `UndefinedValue` if a reserved
    /// value was never resolved, or only to itself; `TableFull` if the
    /// `phi` operands do not fit the operand pool.
    pub fn finish(mut self) -> IrResult<Body> {
        let body = &mut self.body;
        if self.cur != UNSET {
            return bad(IrErrorKind::BadBlock, format!("bb{} has no terminator", self.cur));
        }
        if body.layout.is_empty() {
            return bad(IrErrorKind::BadBlock, "a body needs a block".to_string());
        }
        if let Some(index) = body.blocks.iter().position(|block| block.first == UNSET) {
            return bad(IrErrorKind::BadBlock, format!("bb{index} was created but never begun"));
        }
        room(body.vals.len(), body.pending.len(), "operands")?;
        room(body.words.len(), body.pending.len(), "operand words")?;

        // What each reserved value finally stands for.
        let is_reserved =
            |value: Value| value.tag() == TAG_INST && value.payload() >= RESERVED_BASE;
        for index in 0..body.reserved.len() {
            let mut target = body.reserved[index].target;
            let mut steps = 0;
            while is_reserved(target) {
                steps += 1;
                if steps > body.reserved.len() {
                    return bad(
                        IrErrorKind::UndefinedValue,
                        format!("reserved value {index} is resolved only to itself"),
                    );
                }
                target = body.reserved[(target.payload() - RESERVED_BASE) as usize].target;
            }
            if target.is_none() {
                return bad(
                    IrErrorKind::UndefinedValue,
                    format!(
                        "reserved value {index} of type {:?} was never defined",
                        body.reserved[index].ty
                    ),
                );
            }
            body.reserved[index].target = target;
        }

        // Every operand that named a reserved value names its definition,
        // and the definition learns of the use.
        for at in 0..body.fixups.len() {
            let fixup = body.fixups[at];
            let operand = match fixup.place {
                Place::Inline(slot) => &mut body.insts[fixup.user as usize].ops[usize::from(slot)],
                Place::Val(index) => &mut body.vals[index as usize],
                Place::Incoming(index) => &mut body.pending[index as usize].value,
            };
            let target = body.reserved[(operand.payload() - RESERVED_BASE) as usize].target;
            *operand = target;
            let index = target.payload();
            match target.tag() {
                TAG_INST => {
                    let outside = !body.blocks[fixup.block as usize].contains(index);
                    let next = !fixup.phi && !outside && index + 1 == fixup.user;
                    body.uses[index as usize].add(outside, next);
                }
                TAG_ARG => body.arg_uses[index as usize].add(fixup.block != self.entry, false),
                _ => {}
            }
        }
        body.fixups.clear();

        name_debug_values(body);

        // The incoming lists move to the pools, grouped by `phi`: count,
        // lay out in instruction order, fill.
        for at in 0..body.pending.len() {
            let count = &mut body.insts[body.pending[at].phi as usize].ops[1];
            *count = word(count.bits() + 1);
        }
        let mut vals = body.vals.len() as u32;
        let mut words = body.words.len() as u32;
        for id in &body.layout {
            let block = body.blocks[id.index()];
            for inst in &mut body.insts[block.first as usize..(block.first + block.phis) as usize] {
                let count = inst.ops[1].bits();
                inst.ops = [word(vals), word(0), word(words)];
                vals += count;
                words += count;
            }
        }
        body.vals.resize(vals as usize, Value::NONE);
        body.words.resize(words as usize, 0);
        for at in 0..body.pending.len() {
            let incoming = body.pending[at];
            let ops = &mut body.insts[incoming.phi as usize].ops;
            let filled = ops[1].bits();
            body.vals[(ops[0].bits() + filled) as usize] = incoming.value;
            body.words[(ops[2].bits() + filled) as usize] = incoming.block;
            ops[1] = word(filled + 1);
        }
        body.pending.clear();
        Ok(self.body)
    }
}

/// The end of `finish` for the debug records (§6.6): records and
/// declarations name what a reserved value was defined as, a declaration
/// whose address turns out to be a slot becomes a slot variable, and the
/// values declarations name are marked in the use facts. Clears the
/// reserved values.
fn name_debug_values(body: &mut Body) {
    // Debug records and declarations name what a reserved value was
    // defined as, and do not use it. A declaration whose address
    // turns out to be a slot is a slot variable.
    if !body.reserved.is_empty() {
        let reserved = &body.reserved;
        let resolve = |value: &mut Value| {
            if value.tag() == TAG_INST && value.payload() >= RESERVED_BASE {
                *value = reserved[(value.payload() - RESERVED_BASE) as usize].target;
            }
        };
        for marker in &mut body.markers {
            resolve(&mut marker.ptr);
        }
        for record in &body.dbg_records {
            let count =
                usize::from(record.count) + usize::from(record.kind == DbgRecordKind::Assign);
            body.vals[record.vals as usize..][..count].iter_mut().for_each(resolve);
        }
        let slot_vars = &mut body.slot_vars;
        body.declares.retain_mut(|declare| {
            resolve(&mut declare.address);
            if declare.address.is_none() || declare.address.tag() != TAG_SLOT {
                return true;
            }
            let slot = SlotId::new(declare.address.payload());
            slot_vars.push(SlotVar {
                slot,
                var: declare.var,
                loc: declare.loc,
                expr: declare.expr,
            });
            false
        });
    }
    body.reserved.clear();
    for declare in &body.declares {
        let index = declare.address.payload() as usize;
        match declare.address.tag() {
            _ if declare.address.is_none() => {}
            TAG_INST => body.uses[index].declare(),
            TAG_ARG => body.arg_uses[index].declare(),
            _ => {}
        }
    }
}
