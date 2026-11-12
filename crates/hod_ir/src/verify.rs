//! `Body::verify` (`spec/ir.md` §5.1): every rule a body must satisfy that
//! can be decided from the body and its module.
//!
//! The builder keeps a body's tables sound with a few compares per operand
//! and leaves everything that needs the type table to this pass. Tests run
//! it on every body they build; a producer under test runs it on every
//! body it hands over. It is the specification of "well formed" for the
//! code generator: codegen may assume everything checked here.
//!
//! Dominance (§5.8) is checked as LLVM defines it, with a dominator tree
//! built only for a body that has a use outside its definition's block
//! (`crate::dom`). Not checked: that a symbol or inline-assembly id
//! exists, which needs tables this crate does not have yet.
//!
//! Debug information (§6.8): the declarations and records are checked
//! against the body always, and against the module's debug tables when
//! the module has any.

use std::fmt::Display;

use crate::body::{Body, CallData, Clause, DbgRecordKind, Inst, UNSET, UseFacts};
use crate::constant::Const;
use crate::debug::expr::{DiOp, ExprId};
use crate::debug::node::DiNode;
use crate::dom::Dominators;
use crate::error::{IrError, IrErrorKind, IrResult};
use crate::ids::{BlockId, ConstId, DiId, LocId, TypeId};
use crate::inst::{AtomicOrdering, InstData, MemFlags, Opcode, PTRADD_POOLED, RmwOp};
use crate::intrinsic::{CODE_OTHER, Intrinsic};
use crate::module::Module;
use crate::symbol::SymbolKind;
use crate::types::{TypeKind, TypeTable};
use crate::value::{InstId, TAG_ASM, Value, ValueKind};

use IrErrorKind::{
    BadBlock, BadIndex, InvalidDebugInfo, InvalidOperand, InvalidType, NotDominated, TypeMismatch,
    UnsizedType, UseFactsMismatch,
};

impl Body {
    /// Checks every structural and type rule of §5 and that the recorded
    /// use facts are the ones the instructions imply (§5.6).
    ///
    /// # Errors
    /// The first violation found: `BadBlock` for block structure
    /// (terminators, `phi` and landing-pad placement, `phi` lists against
    /// predecessors), `InvalidOperand` for an operand, id or count that is
    /// out of range or not allowed in its position, `TypeMismatch` for
    /// types that must agree and do not, `InvalidType` or `UnsizedType`
    /// for a type of the wrong class, `BadIndex` for an aggregate offset
    /// or shuffle index that names nothing, `UseFactsMismatch` for use
    /// facts that differ from the recomputed ones, `InvalidDebugInfo`
    /// for a debug declaration, record or location that breaks a rule of
    /// §6.8.
    ///
    /// # Panics
    /// May panic if `module` is not the module the body was built for.
    pub fn verify(&self, module: &Module) -> IrResult<()> {
        let v = Verifier { body: self, module, types: &module.types };
        v.structure()?;
        for block in self.blocks() {
            for raw in block.inst_range() {
                v.inst(raw, block.id())?;
            }
        }
        v.control_flow()?;
        v.side_tables()?;
        v.debug_records()?;
        if !module.debug.is_empty() {
            v.debug_ids()?;
        }
        v.use_facts()
    }
}

/// The scalar type of `ty` and, for a vector, its length and whether it
/// is scalable.
fn shape(types: &TypeTable, ty: TypeId) -> (TypeId, Option<(u32, bool)>) {
    match types.kind(ty) {
        TypeKind::Vector { elem, len } => (elem, Some((len, false))),
        TypeKind::ScalableVector { elem, min_len } => (elem, Some((min_len, true))),
        _ => (ty, None),
    }
}

/// Whether `op` can convert `src` to `dst`: LLVM's `castIsValid`.
fn cast_is_valid(types: &TypeTable, op: Opcode, src: TypeId, dst: TypeId) -> bool {
    let (s, s_vec) = shape(types, src);
    let (d, d_vec) = shape(types, dst);
    let same = s_vec == d_vec;
    let bits = |ty| types.size_in_bits_of(ty);
    let (int, float, ptr) = (|ty| types.is_int(ty), |ty| types.is_float(ty), |ty| types.is_ptr(ty));
    match op {
        Opcode::Trunc => same && int(s) && int(d) && bits(s) > bits(d),
        Opcode::ZExt | Opcode::SExt => same && int(s) && int(d) && bits(s) < bits(d),
        Opcode::FPTrunc => same && float(s) && float(d) && bits(s) > bits(d),
        Opcode::FPExt => same && float(s) && float(d) && bits(s) < bits(d),
        Opcode::FPToUI | Opcode::FPToSI => same && float(s) && int(d),
        Opcode::UIToFP | Opcode::SIToFP => same && int(s) && float(d),
        Opcode::PtrToInt => same && ptr(s) && int(d),
        // The result is exactly as wide as an address of that space.
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
                // Pointers: the same address space, and a vector only to a
                // vector of the same length or from and to one element.
                return s == d
                    && match (s_vec, d_vec) {
                        (Some(a), Some(b)) => a == b,
                        (Some(one), None) | (None, Some(one)) => one == (1, false),
                        (None, None) => true,
                    };
            }
            let value = |ty| int(ty) || float(ty);
            let scalable = |vec: Option<(u32, bool)>| vec.is_some_and(|(_, scalable)| scalable);
            value(s) && value(d) && bits(src) == bits(dst) && scalable(s_vec) == scalable(d_vec)
        }
        _ => false,
    }
}

/// Whether a value of type `ty` can be accessed atomically: an integer,
/// pointer, float or vector of them whose size in bits is a power of two
/// and at least eight (LLVM's rule). A scalable vector counts with its
/// minimum size, as LLVM counts it.
fn atomic_type(types: &TypeTable, ty: TypeId) -> bool {
    let (scalar, vec) = shape(types, ty);
    let class = types.is_int(scalar) || types.is_ptr(scalar) || types.is_float(scalar);
    let bits = types.size_in_bits_of(scalar) * vec.map_or(1, |(len, _)| u64::from(len));
    class && bits >= 8 && bits.is_power_of_two()
}

/// Whether `agg` has, at any depth, a field or element of type `field`
/// at byte `offset`.
fn field_at(types: &TypeTable, agg: TypeId, offset: u64, field: TypeId) -> bool {
    match types.kind(agg) {
        TypeKind::Struct { fields, .. } => {
            let Some(offsets) = types.field_offsets(agg) else { return false };
            fields.iter().zip(offsets).any(|(&inner, &at)| {
                at <= offset
                    && offset - at <= types.size_of(inner)
                    && ((at == offset && inner == field)
                        || field_at(types, inner, offset - at, field))
            })
        }
        TypeKind::Array { elem, len } => {
            let stride = types.alloc_size_of(elem);
            // Elements without a size all lie at the array's start.
            let (index, rest) = match offset.checked_div(stride) {
                Some(index) => (index, offset % stride),
                None => (0, offset),
            };
            index < len && ((rest == 0 && elem == field) || field_at(types, elem, rest, field))
        }
        _ => false,
    }
}

/// An error about `block`.
#[cold]
fn fail_block<T>(block: BlockId, what: impl Display) -> IrResult<T> {
    Err(IrError::new(BadBlock, format!("{block:?}: {what}")))
}

struct Verifier<'a> {
    body: &'a Body,
    module: &'a Module,
    types: &'a TypeTable,
}

impl Verifier<'_> {
    /// An error about instruction `id`.
    #[cold]
    fn fail<T>(&self, kind: IrErrorKind, id: u32, what: impl Display) -> IrResult<T> {
        let name = self.body.insts.get(id as usize).map_or("?", |inst| inst.op.name());
        Err(IrError::new(kind, format!("%{id} ({name}): {what}")))
    }

    fn shape(&self, ty: TypeId) -> (TypeId, Option<(u32, bool)>) {
        shape(self.types, ty)
    }

    /// The tables agree with each other.
    fn tables(&self) -> IrResult<()> {
        let body = self.body;
        let table = |ok: bool, what: &str| {
            if ok { Ok(()) } else { Err(IrError::new(InvalidOperand, what.to_string())) }
        };
        table(body.fn_ty.index() < self.types.len(), "the function type is not in the module")?;
        let TypeKind::Function { params, .. } = self.types.kind(body.fn_ty) else {
            return Err(IrError::new(InvalidType, "a body's type must be a function type"));
        };
        table(params.len() == body.arg_count as usize, "argument count differs from the type's")?;
        if body.arg_uses.len() != params.len() || body.uses.len() != body.insts.len() {
            let what = "the use-fact arrays do not match the arguments and instructions";
            return Err(IrError::new(UseFactsMismatch, what));
        }
        table(
            body.locs.is_empty() || body.locs.len() == body.insts.len(),
            "the location array does not match the instructions",
        )?;
        table(
            body.slots.iter().all(|slot| slot.align_log2() <= 32),
            "a stack slot's alignment is over 2^32",
        )?;
        table(
            body.slot_vars.iter().all(|var| var.slot.index() < body.slots.len()),
            "a slot variable names no slot",
        )
    }

    /// The blocks tile the instruction array, `phi`s first, one
    /// terminator last.
    fn structure(&self) -> IrResult<()> {
        self.tables()?;
        let body = self.body;
        let block_error = |what: String| Err(IrError::new(BadBlock, what));
        if body.layout.is_empty() {
            return block_error("a body needs a block".to_string());
        }
        if body.layout.len() != body.blocks.len() {
            return block_error("the layout does not list every block once".to_string());
        }
        let mut seen = vec![false; body.blocks.len()];
        let mut next = 0u32;
        for &id in &body.layout {
            let Some(block) = body.blocks.get(id.index()) else {
                return block_error(format!("the layout names {id:?}, which does not exist"));
            };
            if std::mem::replace(&mut seen[id.index()], true) {
                return block_error(format!("{id:?} is laid out twice"));
            }
            if block.first != next || block.end == UNSET || block.end <= block.first {
                return fail_block(id, "its instructions do not follow the previous block's");
            }
            if block.end as usize > body.insts.len() || block.phis >= block.end - block.first {
                return fail_block(id, "its instruction range is out of bounds");
            }
            for raw in block.first..block.end {
                let op = body.insts[raw as usize].op;
                let is_phi = raw < block.first + block.phis;
                if (op == Opcode::Phi) != is_phi {
                    return fail_block(id, format!("%{raw}: phis must start the block"));
                }
                if op.is_terminator() != (raw + 1 == block.end) {
                    let what = "a block ends at its first and only terminator";
                    return fail_block(id, format!("%{raw} ({}): {what}", op.name()));
                }
            }
            next = block.end;
        }
        if next as usize != body.insts.len() {
            return block_error("instructions follow the last block".to_string());
        }
        Ok(())
    }

    // ---- operands ----

    fn type_id(&self, id: u32, ty: TypeId) -> IrResult<()> {
        if ty.index() >= self.types.len() {
            return self.fail(InvalidOperand, id, format!("{ty:?} is not a type of the module"));
        }
        Ok(())
    }

    fn block_id(&self, id: u32, raw: u32) -> IrResult<()> {
        if raw as usize >= self.body.blocks.len() {
            return self.fail(InvalidOperand, id, format!("bb{raw} does not exist"));
        }
        Ok(())
    }

    fn const_id(&self, id: u32, constant: ConstId) -> IrResult<TypeId> {
        if constant.index() >= self.module.consts.len() {
            return self.fail(InvalidOperand, id, format!("{constant:?} is not in the module"));
        }
        Ok(self.module.consts.ty(constant))
    }

    /// The type of operand `value` of instruction `id`, if it names
    /// something that has one.
    fn value(&self, id: u32, value: Value) -> IrResult<TypeId> {
        let body = self.body;
        match value.kind() {
            ValueKind::Inst(def) => match body.insts.get(def.index()) {
                Some(inst) if inst.ty != TypeId::VOID => Ok(inst.ty),
                Some(_) => self.fail(InvalidOperand, id, format!("{value:?} has no result")),
                None => self.fail(InvalidOperand, id, format!("{value:?} does not exist")),
            },
            ValueKind::Arg(index) => match body.arg_types(self.module).get(index as usize) {
                Some(&ty) => Ok(ty),
                None => self.fail(InvalidOperand, id, format!("{value:?} does not exist")),
            },
            ValueKind::Const(constant) => self.const_id(id, constant),
            ValueKind::Sym(_) => Ok(TypeId::PTR),
            ValueKind::Slot(slot) if slot.index() < body.slots.len() => Ok(TypeId::PTR),
            ValueKind::Slot(_) => {
                self.fail(InvalidOperand, id, format!("{value:?} does not exist"))
            }
            ValueKind::Asm(_) => {
                self.fail(InvalidOperand, id, "inline assembly can only be called")
            }
            ValueKind::None => self.fail(InvalidOperand, id, "an operand is missing"),
        }
    }

    /// `value` must be a (scalar) pointer.
    fn pointer(&self, id: u32, value: Value) -> IrResult<()> {
        let ty = self.value(id, value)?;
        if !self.types.is_ptr(ty) {
            return self.fail(InvalidType, id, format!("{value:?} must be a pointer, not {ty:?}"));
        }
        Ok(())
    }

    fn integer(&self, id: u32, value: Value) -> IrResult<TypeId> {
        let ty = self.value(id, value)?;
        if !self.types.is_int(ty) {
            return self.fail(InvalidType, id, format!("{value:?} must be an integer, not {ty:?}"));
        }
        Ok(ty)
    }

    fn same(&self, id: u32, what: &str, found: TypeId, expected: TypeId) -> IrResult<()> {
        if found != expected {
            let what = format!("{what} has type {found:?} where {expected:?} is required");
            return self.fail(TypeMismatch, id, what);
        }
        Ok(())
    }

    fn void(&self, id: u32, ty: TypeId) -> IrResult<()> {
        if ty != TypeId::VOID {
            return self.fail(InvalidType, id, "the instruction has no result: its type is void");
        }
        Ok(())
    }

    /// `[start, start + len)` must lie in a pool of `pool` entries.
    fn range(&self, id: u32, start: u32, len: u64, pool: usize, what: &str) -> IrResult<()> {
        if u64::from(start) + len > pool as u64 {
            return self.fail(InvalidOperand, id, format!("its {what} lie outside the pool"));
        }
        Ok(())
    }

    fn atomic(&self, id: u32, ty: TypeId, ordering: Option<AtomicOrdering>) -> IrResult<()> {
        if ordering.is_some() && !atomic_type(self.types, ty) {
            let what = format!("{ty:?} cannot be accessed atomically");
            return self.fail(InvalidType, id, what);
        }
        Ok(())
    }

    fn ordering(&self, id: u32, ok: bool, ordering: AtomicOrdering) -> IrResult<()> {
        if !ok {
            let what = format!("ordering {} is not allowed here", ordering.name());
            return self.fail(InvalidOperand, id, what);
        }
        Ok(())
    }

    // ---- one instruction ----

    #[allow(clippy::too_many_lines)]
    fn inst(&self, id: u32, block: BlockId) -> IrResult<()> {
        let body = self.body;
        let types = self.types;
        let d: &InstData = &body.insts[id as usize];
        let [a, b, c] = d.ops;
        let ty = d.ty;
        self.type_id(id, ty)?;
        let (scalar, vec) = self.shape(ty);
        match d.op {
            op if op.is_binary() => {
                let class =
                    if op.is_int_binary() { types.is_int(scalar) } else { types.is_float(scalar) };
                if !class {
                    return self.fail(InvalidType, id, format!("{ty:?} is the wrong kind of type"));
                }
                self.same(id, "the left operand", self.value(id, a)?, ty)?;
                self.same(id, "the right operand", self.value(id, b)?, ty)
            }
            Opcode::FNeg => {
                if !types.is_float(scalar) {
                    return self.fail(InvalidType, id, format!("{ty:?} is not floating point"));
                }
                self.same(id, "the operand", self.value(id, a)?, ty)
            }
            Opcode::Freeze => self.same(id, "the operand", self.value(id, a)?, ty),
            Opcode::ICmp | Opcode::FCmp => {
                let lhs = self.value(id, a)?;
                self.same(id, "the right operand", self.value(id, b)?, lhs)?;
                let (operand, operand_vec) = self.shape(lhs);
                let class = if d.op == Opcode::ICmp {
                    types.is_int(operand) || types.is_ptr(operand)
                } else {
                    types.is_float(operand)
                };
                if !class {
                    return self.fail(InvalidType, id, format!("{lhs:?} cannot be compared so"));
                }
                if scalar != TypeId::I1 || vec != operand_vec {
                    let what = "the result is i1, or a vector of i1 as long as the operands";
                    return self.fail(TypeMismatch, id, what);
                }
                Ok(())
            }
            op if op.is_cast() => {
                let src = self.value(id, a)?;
                if !cast_is_valid(types, op, src, ty) {
                    let what = format!("{src:?} cannot be converted to {ty:?} this way");
                    return self.fail(InvalidType, id, what);
                }
                Ok(())
            }
            Opcode::Load => {
                let mem = MemFlags::from_raw(d.flags);
                self.pointer(id, a)?;
                if !types.is_sized(ty) {
                    return self.fail(UnsizedType, id, format!("{ty:?} cannot be loaded"));
                }
                if let Some(ordering) = mem.ordering() {
                    self.ordering(id, ordering.valid_for_load(), ordering)?;
                }
                self.atomic(id, ty, mem.ordering())
            }
            Opcode::Store => {
                let mem = MemFlags::from_raw(d.flags);
                self.void(id, ty)?;
                let stored = self.value(id, a)?;
                self.pointer(id, b)?;
                if !types.is_sized(stored) {
                    return self.fail(UnsizedType, id, format!("{stored:?} cannot be stored"));
                }
                if let Some(ordering) = mem.ordering() {
                    self.ordering(id, ordering.valid_for_store(), ordering)?;
                }
                self.atomic(id, stored, mem.ordering())
            }
            Opcode::Alloca => {
                self.integer(id, a)?;
                if !types.is_ptr(ty) {
                    return self.fail(InvalidType, id, "the result is a pointer");
                }
                if d.sub > 32 {
                    return self.fail(InvalidOperand, id, "the alignment is over 2^32");
                }
                Ok(())
            }
            Opcode::Fence => {
                self.void(id, ty)?;
                let ordering = AtomicOrdering::from_raw(d.sub);
                let known = usize::from(d.sub) < AtomicOrdering::ALL.len();
                self.ordering(id, known && ordering.valid_for_fence(), ordering)
            }
            Opcode::CmpXchg => {
                let (mem, failure, _) = MemFlags::unpack_cmpxchg(d.flags);
                self.pointer(id, a)?;
                let expected = self.value(id, b)?;
                self.same(id, "the new value", self.value(id, c)?, expected)?;
                if !types.is_int(expected) && !types.is_ptr(expected) {
                    let what = format!("{expected:?} is not an integer or a pointer");
                    return self.fail(InvalidType, id, what);
                }
                if types.struct_fields(ty) != Some(&[expected, TypeId::I1]) {
                    let what = "the result is a struct of the operand type and i1";
                    return self.fail(TypeMismatch, id, what);
                }
                let Some(success) = mem.ordering() else {
                    return self.fail(InvalidOperand, id, "a cmpxchg needs a success ordering");
                };
                self.ordering(id, success.valid_for_rmw(), success)?;
                self.ordering(id, failure.valid_for_cmpxchg_failure(), failure)?;
                self.atomic(id, expected, Some(success))
            }
            Opcode::AtomicRmw => {
                let mem = MemFlags::from_raw(d.flags);
                let op = RmwOp::from_raw(d.sub);
                self.pointer(id, a)?;
                self.same(id, "the operand", self.value(id, b)?, ty)?;
                let class = match op {
                    RmwOp::Xchg => types.is_int(ty) || types.is_float(ty) || types.is_ptr(ty),
                    op if op.is_float() => {
                        types.is_float(scalar) && !vec.is_some_and(|(_, scalable)| scalable)
                    }
                    _ => types.is_int(ty),
                };
                if !class || usize::from(d.sub) >= RmwOp::ALL.len() {
                    let what = format!("{ty:?} is not a type {} works on", op.name());
                    return self.fail(InvalidType, id, what);
                }
                let Some(ordering) = mem.ordering() else {
                    return self.fail(InvalidOperand, id, "an atomicrmw needs an ordering");
                };
                self.ordering(id, ordering.valid_for_rmw(), ordering)?;
                self.atomic(id, ty, Some(ordering))
            }
            Opcode::PtrAdd => self.ptr_add(id, d),
            Opcode::ExtractValue | Opcode::InsertValue => {
                let agg = self.value(id, a)?;
                if !matches!(types.kind(agg), TypeKind::Struct { .. } | TypeKind::Array { .. }) {
                    return self.fail(InvalidType, id, format!("{agg:?} is not an aggregate"));
                }
                if types.is_scalable(agg) {
                    return self.fail(InvalidType, id, "a scalable aggregate has no fixed offsets");
                }
                let (field, offset) = if d.op == Opcode::ExtractValue {
                    (ty, u64::from(b.bits()) | (u64::from(c.bits()) << 32))
                } else {
                    self.same(id, "the aggregate", agg, ty)?;
                    (self.value(id, b)?, u64::from(c.bits()))
                };
                if !field_at(types, agg, offset, field) {
                    let what = format!("{agg:?} has no field of type {field:?} at byte {offset}");
                    return self.fail(BadIndex, id, what);
                }
                Ok(())
            }
            Opcode::ExtractElement | Opcode::InsertElement => {
                let vector = self.value(id, a)?;
                let (elem, Some(_)) = self.shape(vector) else {
                    return self.fail(InvalidType, id, format!("{vector:?} is not a vector"));
                };
                if d.op == Opcode::ExtractElement {
                    self.integer(id, b)?;
                    self.same(id, "the result", ty, elem)
                } else {
                    self.integer(id, c)?;
                    self.same(id, "the result", ty, vector)?;
                    self.same(id, "the new element", self.value(id, b)?, elem)
                }
            }
            Opcode::ShuffleVector => {
                let first = self.value(id, a)?;
                self.same(id, "the second vector", self.value(id, b)?, first)?;
                let (elem, Some((len, scalable))) = self.shape(first) else {
                    return self.fail(InvalidType, id, format!("{first:?} is not a vector"));
                };
                self.range(id, c.bits(), 1, body.words.len(), "mask")?;
                let mask_len = body.words[c.bits() as usize];
                self.range(id, c.bits(), u64::from(mask_len) + 1, body.words.len(), "mask")?;
                if (scalar, vec) != (elem, Some((mask_len, scalable))) {
                    let what = "the result has the operands' element type and the mask's length";
                    return self.fail(TypeMismatch, id, what);
                }
                let mask = body.word_list(c.bits() + 1, mask_len);
                let limit = if scalable { 1 } else { 2 * u64::from(len) };
                if mask.iter().any(|&m| m != crate::SHUFFLE_UNDEF && u64::from(m) >= limit) {
                    return self.fail(BadIndex, id, "a mask entry selects no element");
                }
                Ok(())
            }
            Opcode::Phi => {
                if ty == TypeId::VOID {
                    return self.fail(InvalidType, id, "a phi has a value");
                }
                let count = u64::from(b.bits());
                self.range(id, a.bits(), count, body.vals.len(), "incoming values")?;
                self.range(id, c.bits(), count, body.words.len(), "incoming blocks")?;
                for &value in body.val_list(a.bits(), b.bits()) {
                    self.same(id, "an incoming value", self.value(id, value)?, ty)?;
                }
                for &raw in body.word_list(c.bits(), b.bits()) {
                    self.block_id(id, raw)?;
                }
                Ok(())
            }
            Opcode::Select => {
                let cond = self.value(id, a)?;
                self.same(id, "the true value", self.value(id, b)?, ty)?;
                self.same(id, "the false value", self.value(id, c)?, ty)?;
                let (cond_scalar, cond_vec) = self.shape(cond);
                if cond_scalar != TypeId::I1 || (cond_vec.is_some() && cond_vec != vec) {
                    let what = "the condition is i1, or a vector of i1 as long as the values";
                    return self.fail(TypeMismatch, id, what);
                }
                Ok(())
            }
            Opcode::VaArg => {
                self.pointer(id, a)?;
                if !types.is_sized(ty) {
                    return self.fail(UnsizedType, id, format!("{ty:?} cannot be an argument"));
                }
                Ok(())
            }
            Opcode::Call | Opcode::Invoke | Opcode::CallBr => self.call(id, d),
            Opcode::LandingPad => {
                let Some(pad) = body.pads.get(a.bits() as usize) else {
                    return self.fail(InvalidOperand, id, "it has no landing-pad record");
                };
                if pad.inst != id {
                    return self.fail(InvalidOperand, id, "its record belongs to another pad");
                }
                if ty == TypeId::VOID {
                    return self.fail(InvalidType, id, "a landing pad has a value");
                }
                let data = body.blocks[block.index()];
                if id != data.first + data.phis {
                    return fail_block(block, format!("%{id}: a landing pad comes first"));
                }
                let (start, count) = (pad.clauses, u64::from(pad.nclauses));
                self.range(id, start, count, body.clauses.len(), "clauses")?;
                for clause in &body.clauses[start as usize..][..pad.nclauses as usize] {
                    let ok = match *clause {
                        Clause::Catch(constant) => types.is_ptr(self.const_id(id, constant)?),
                        Clause::Filter(constant) => matches!(
                            types.kind(self.const_id(id, constant)?),
                            TypeKind::Array { .. }
                        ),
                    };
                    if !ok {
                        let what = "a catch clause is a pointer and a filter clause an array";
                        return self.fail(InvalidType, id, what);
                    }
                }
                Ok(())
            }
            Opcode::Br => {
                self.void(id, ty)?;
                self.block_id(id, a.bits())
            }
            Opcode::CondBr => {
                self.void(id, ty)?;
                self.same(id, "the condition", self.value(id, a)?, TypeId::I1)?;
                self.block_id(id, b.bits())?;
                self.block_id(id, c.bits())
            }
            Opcode::Switch => {
                self.void(id, ty)?;
                let cond = self.integer(id, a)?;
                let count = c.bits();
                self.range(id, b.bits(), 2 * u64::from(count) + 1, body.words.len(), "cases")?;
                for &raw in body.word_list(b.bits(), count + 1) {
                    self.block_id(id, raw)?;
                }
                let values = body.word_list(b.bits() + 1 + count, count);
                for &raw in values {
                    let constant = ConstId::new(raw);
                    self.same(id, "a case value", self.const_id(id, constant)?, cond)?;
                    let literal = self.module.consts.get(constant);
                    if !matches!(literal, Const::Int { .. } | Const::WideInt { .. }) {
                        return self.fail(InvalidOperand, id, "a case value is a literal integer");
                    }
                }
                let mut sorted = values.to_vec();
                sorted.sort_unstable();
                if sorted.windows(2).any(|pair| pair[0] == pair[1]) {
                    return self.fail(InvalidOperand, id, "two cases have the same value");
                }
                Ok(())
            }
            Opcode::IndirectBr => {
                self.void(id, ty)?;
                self.pointer(id, a)?;
                self.range(id, b.bits(), u64::from(c.bits()), body.words.len(), "targets")?;
                for &raw in body.word_list(b.bits(), c.bits()) {
                    self.block_id(id, raw)?;
                }
                Ok(())
            }
            Opcode::Ret => {
                self.void(id, ty)?;
                let expected = body.ret_type(self.module);
                match (a.is_none(), expected == TypeId::VOID) {
                    (true, true) => Ok(()),
                    (false, false) => {
                        self.same(id, "the returned value", self.value(id, a)?, expected)
                    }
                    (true, false) => self.fail(TypeMismatch, id, "the function returns a value"),
                    (false, true) => self.fail(TypeMismatch, id, "the function returns void"),
                }
            }
            Opcode::Resume => {
                self.void(id, ty)?;
                let value = self.value(id, a)?;
                match body.pads.first() {
                    Some(pad) => {
                        let pad_ty = body.insts.get(pad.inst as usize).map_or(value, |pad| pad.ty);
                        self.same(id, "the exception", value, pad_ty)
                    }
                    None => Ok(()),
                }
            }
            Opcode::Unreachable => self.void(id, ty),
            // The funclet instructions have nothing to check.
            _ => Ok(()),
        }
    }

    fn ptr_add(&self, id: u32, d: &InstData) -> IrResult<()> {
        let body = self.body;
        let [a, b, c] = d.ops;
        let (base, indices): (Value, &[Value]) = if d.flags & PTRADD_POOLED == 0 {
            (a, &[])
        } else {
            let count = u64::from(b.bits());
            self.range(id, a.bits(), count, body.vals.len(), "operands")?;
            self.range(id, c.bits(), 2 * count, body.words.len(), "scales")?;
            match body.val_list(a.bits(), b.bits()) {
                [base, indices @ ..] => (*base, indices),
                [] => return self.fail(InvalidOperand, id, "it has no base"),
            }
        };
        let (base_scalar, mut vec) = self.shape(self.value(id, base)?);
        if !self.types.is_ptr(base_scalar) {
            return self.fail(InvalidType, id, format!("the base {base:?} is not a pointer"));
        }
        for &index in indices {
            let (index_scalar, index_vec) = self.shape(self.value(id, index)?);
            if !self.types.is_int(index_scalar) {
                return self.fail(
                    InvalidType,
                    id,
                    format!("the index {index:?} is not an integer"),
                );
            }
            if index_vec.is_some() {
                if vec.is_some() && vec != index_vec {
                    return self.fail(TypeMismatch, id, "vector operands differ in length");
                }
                vec = index_vec;
            }
        }
        if self.shape(d.ty) != (base_scalar, vec) {
            let what = "the result is the base's pointer type, a vector if any operand is";
            return self.fail(TypeMismatch, id, what);
        }
        Ok(())
    }

    fn call(&self, id: u32, d: &InstData) -> IrResult<()> {
        let body = self.body;
        let types = self.types;
        let [a, b, c] = d.ops;
        let Some(call) = body.calls.get(c.bits() as usize) else {
            return self.fail(InvalidOperand, id, "it has no call record");
        };
        if call.inst != id {
            return self.fail(InvalidOperand, id, "its record belongs to another call");
        }
        self.range(id, a.bits(), u64::from(b.bits()), body.vals.len(), "operands")?;
        let vals = body.val_list(a.bits(), b.bits());
        let (start, count) = (call.bundles, u64::from(call.nbundles));
        self.range(id, start, count, body.bundles.len(), "bundles")?;
        let bundles = &body.bundles[start as usize..][..usize::from(call.nbundles)];
        let mut expected = u64::from(a.bits()) + 1 + u64::from(call.nargs);
        for bundle in bundles {
            let tag = u64::from(bundle.tag_start) + u64::from(bundle.tag_len);
            let in_names = tag <= body.names.len() as u64
                && body.names.is_char_boundary(bundle.tag_start as usize)
                && body.names.is_char_boundary(tag as usize);
            if u64::from(bundle.first) != expected || !in_names {
                return self.fail(InvalidOperand, id, "a bundle's inputs or tag are misplaced");
            }
            expected += u64::from(bundle.count);
        }
        if expected != u64::from(a.bits()) + u64::from(b.bits()) {
            let what = "its operands are not the callee, the arguments and the bundle inputs";
            return self.fail(InvalidOperand, id, what);
        }
        let args = &vals[1..=call.nargs as usize];

        self.type_id(id, call.fn_ty)?;
        let TypeKind::Function { ret, params, variadic } = types.kind(call.fn_ty) else {
            return self.fail(InvalidType, id, format!("{:?} is not a function type", call.fn_ty));
        };
        self.same(id, "the result", d.ty, ret)?;
        if args.len() < params.len() || (args.len() > params.len() && !variadic) {
            let what = format!("{} arguments for {} parameters", args.len(), params.len());
            return self.fail(InvalidOperand, id, what);
        }
        for (index, &arg) in args.iter().enumerate() {
            let arg_ty = self.value(id, arg)?;
            if let Some(&param) = params.get(index) {
                self.same(id, "an argument", arg_ty, param)?;
            }
        }
        for &input in &vals[1 + args.len()..] {
            self.value(id, input)?;
        }
        if call.attrs != UNSET {
            let count = u64::from(call.nargs) + 1;
            self.range(id, call.attrs, count, body.param_attrs.len(), "attributes")?;
        }
        self.callee(id, call, vals[0])?;

        match d.op {
            Opcode::Invoke => {
                self.block_id(id, call.normal)?;
                self.block_id(id, call.unwind)
            }
            Opcode::CallBr => {
                let list = call.unwind;
                self.range(id, list, 2, body.words.len(), "destinations")?;
                let count = body.words[list as usize];
                self.range(id, list, u64::from(count) + 1, body.words.len(), "destinations")?;
                let dests = body.word_list(list + 1, count);
                if dests.first() != Some(&call.normal) {
                    return self.fail(InvalidOperand, id, "its fallthrough is recorded twice");
                }
                dests.iter().try_for_each(|&raw| self.block_id(id, raw))
            }
            _ => Ok(()),
        }
    }

    fn callee(&self, id: u32, call: &CallData, callee: Value) -> IrResult<()> {
        let body = self.body;
        if call.intrinsic == 0 {
            if callee.tag() == TAG_ASM {
                return Ok(());
            }
            return self.pointer(id, callee);
        }
        if !callee.is_none() {
            return self.fail(InvalidOperand, id, "an intrinsic call has no callee value");
        }
        if call.intrinsic == CODE_OTHER {
            self.range(id, call.extra, 2, body.words.len(), "name")?;
            let start = body.words[call.extra as usize] as usize;
            let len = body.words[call.extra as usize + 1] as usize;
            let name = body.names.get(start..).and_then(|rest| rest.get(..len));
            if !name.is_some_and(|name| name.starts_with("llvm.")) || call.noverloads != 0 {
                return self.fail(InvalidOperand, id, "an intrinsic's name starts with llvm.");
            }
            return Ok(());
        }
        let Some(intrinsic) = Intrinsic::from_code(call.intrinsic) else {
            return self.fail(InvalidOperand, id, "it names no intrinsic");
        };
        let count = usize::from(call.noverloads);
        self.range(id, call.extra, count as u64, body.types.len(), "overload types")?;
        let overloads = &body.types[call.extra as usize..][..count];
        if count != intrinsic.overload_count() {
            let what = format!("{} has {} overload types", intrinsic.name(), count);
            return self.fail(InvalidOperand, id, what);
        }
        match intrinsic.overloads_from(self.types, call.fn_ty) {
            Some(derived) if derived[..count] == *overloads => Ok(()),
            _ => {
                let what = format!(
                    "the overload types of {} are not the ones its function type gives",
                    intrinsic.name()
                );
                self.fail(TypeMismatch, id, what)
            }
        }
    }

    // ---- across blocks ----

    /// `phi` lists against predecessors, the entry block, landing pads.
    fn control_flow(&self) -> IrResult<()> {
        let body = self.body;
        let mut preds: Vec<Vec<u32>> = vec![Vec::new(); body.blocks.len()];
        // How many of a block's incoming edges are an invoke's unwind edge.
        let mut unwinds = vec![0usize; body.blocks.len()];
        for block in body.blocks() {
            for successor in block.successors() {
                preds[successor.index()].push(block.id().raw());
            }
            if let Some(unwind) = block.terminator().call().and_then(|call| call.unwind_dest()) {
                unwinds[unwind.index()] += 1;
                let dest = body.block(unwind);
                let first = dest.non_phis().next().map(Inst::opcode);
                let handler = matches!(
                    first,
                    Some(Opcode::LandingPad | Opcode::CatchSwitch | Opcode::CleanupPad)
                );
                if !handler {
                    let what = format!("{unwind:?} is unwound to and has no landing pad");
                    return fail_block(block.id(), what);
                }
            }
        }
        let entry = body.entry();
        if !preds[entry.index()].is_empty() || body.block(entry).has_phis() {
            return fail_block(entry, "the entry block has no predecessors and no phis");
        }
        let mut pad_ty = None;
        for block in body.blocks() {
            let incoming = &mut preds[block.id().index()];
            incoming.sort_unstable();
            if let Some(first) = block.non_phis().next()
                && first.opcode() == Opcode::LandingPad
            {
                if unwinds[block.id().index()] != incoming.len() {
                    let what = "a landing-pad block is entered only by unwinding from an invoke";
                    return fail_block(block.id(), what);
                }
                if *pad_ty.get_or_insert(first.ty()) != first.ty() {
                    let what = "landing pads of one function have one type";
                    return self.fail(TypeMismatch, first.id().raw(), what);
                }
            }
            for phi in block.phis() {
                let crate::InstKind::Phi(list) = phi.kind() else { continue };
                let mut pairs: Vec<(u32, Value)> =
                    list.incoming().map(|(value, from)| (from.raw(), value)).collect();
                pairs.sort_unstable_by_key(|&(from, value)| (from, value.bits()));
                if !pairs.iter().map(|&(from, _)| from).eq(incoming.iter().copied()) {
                    let what = format!(
                        "%{}: a phi has one entry for each edge into its block",
                        phi.id().raw()
                    );
                    return fail_block(block.id(), what);
                }
                if pairs.windows(2).any(|pair| pair[0].0 == pair[1].0 && pair[0].1 != pair[1].1) {
                    let what = "a predecessor is listed with two different values";
                    return self.fail(InvalidOperand, phi.id().raw(), what);
                }
            }
        }
        Ok(())
    }

    /// The call and landing-pad tables list exactly the instructions that
    /// point at them, in layout order.
    fn side_tables(&self) -> IrResult<()> {
        let body = self.body;
        let ordered = |insts: &mut dyn Iterator<Item = u32>, call: bool| {
            let mut previous = None;
            insts.enumerate().all(|(index, raw)| {
                let rising = previous.replace(raw).is_none_or(|before| before < raw);
                rising
                    && body.insts.get(raw as usize).is_some_and(|inst| {
                        let (kind, at) = if call {
                            (inst.op.is_call(), inst.ops[2].bits())
                        } else {
                            (inst.op == Opcode::LandingPad, inst.ops[0].bits())
                        };
                        kind && at as usize == index
                    })
            })
        };
        if !ordered(&mut body.calls.iter().map(|call| call.inst), true) {
            return Err(IrError::new(InvalidOperand, "the call table is not the body's calls"));
        }
        if !ordered(&mut body.pads.iter().map(|pad| pad.inst), false) {
            let what = "the landing-pad table is not the body's landing pads";
            return Err(IrError::new(InvalidOperand, what));
        }
        Ok(())
    }

    /// Whether a debug declaration or record can name `value`: nothing,
    /// or something that exists. Unlike an operand it may be an
    /// instruction without a result, which LLVM's metadata can name too.
    fn debug_value(&self, value: Value) -> bool {
        let body = self.body;
        match value.kind() {
            ValueKind::None | ValueKind::Sym(_) => true,
            ValueKind::Inst(def) => def.index() < body.insts.len(),
            ValueKind::Arg(index) => index < body.arg_count,
            ValueKind::Const(constant) => constant.index() < self.module.consts.len(),
            ValueKind::Slot(slot) => slot.index() < body.slots.len(),
            ValueKind::Asm(_) => false,
        }
    }

    /// §6.8, the rules that need only the body: declarations and records
    /// name values of this body, and records are in position order, each
    /// before an instruction that is not a `phi`.
    fn debug_records(&self) -> IrResult<()> {
        let body = self.body;
        let fail = |what: String| Err(IrError::new(InvalidDebugInfo, what));
        for declare in &body.declares {
            if matches!(declare.address.kind(), ValueKind::Slot(_)) {
                return fail(format!(
                    "the declaration of {:?} at a slot is not a slot variable",
                    declare.var
                ));
            }
            if !self.debug_value(declare.address) {
                let what = "names a value that does not exist";
                return fail(format!(
                    "the declaration of {:?} {what}: {:?}",
                    declare.var, declare.address
                ));
            }
        }
        let mut previous = 0;
        for (index, record) in body.dbg_records.iter().enumerate() {
            let fail = |what: &str| {
                Err(IrError::new(InvalidDebugInfo, format!("debug record {index} {what}")))
            };
            let Some(inst) = body.insts.get(record.before as usize) else {
                return fail("stands before no instruction");
            };
            if inst.op == Opcode::Phi {
                return fail("stands before a phi");
            }
            if record.before < previous {
                return fail("is out of position order");
            }
            previous = record.before;
            let count = usize::from(record.count);
            let assign = record.kind == DbgRecordKind::Assign;
            let Some(values) = body
                .vals
                .get(record.vals as usize..)
                .and_then(|rest| rest.get(..count + usize::from(assign)))
            else {
                return fail("has its values outside the operand pool");
            };
            let shaped = match record.kind {
                DbgRecordKind::Label => count == 0 && !record.arg_list,
                DbgRecordKind::Value | DbgRecordKind::Assign => record.arg_list || count == 1,
            };
            if !shaped {
                return fail("has a number of values its kind cannot have");
            }
            if !values.iter().all(|&value| self.debug_value(value)) {
                return fail("names a value that does not exist");
            }
        }
        Ok(())
    }

    /// §6.8, the rules that need the module's debug tables. Run when the
    /// module has debug information.
    fn debug_ids(&self) -> IrResult<()> {
        let body = self.body;
        let debug = &self.module.debug;
        // The subprogram the function is attached to, if it has one.
        let function = (body.sym.index() < self.module.symbol_count())
            .then(|| self.module.symbol(body.sym).kind())
            .and_then(|kind| match kind {
                SymbolKind::Function { subprogram, .. } => subprogram,
                _ => None,
            });
        // A location is defined and, when the function has a subprogram,
        // is in it or in something inlined into it.
        let location = |loc: LocId, what: &dyn Display| {
            let fail =
                |why: &str| Err(IrError::new(InvalidDebugInfo, format!("{what}: {loc:?} {why}")));
            if debug.location(loc).is_none() {
                return fail("is not a location");
            }
            let owner = debug.function_of(loc);
            if owner.is_none() {
                return fail("does not lead to a subprogram");
            }
            if function.is_some() && owner != function {
                return fail("belongs to another function's subprogram");
            }
            Ok(())
        };
        let mut previous = LocId::NONE;
        for (index, &loc) in body.locs.iter().enumerate() {
            if loc != previous {
                previous = loc;
                if loc != LocId::NONE {
                    location(loc, &format_args!("%{index}"))?;
                }
            }
        }
        // A variable or label is of the right kind, in the subprogram its
        // record's location is in, and its expression exists and names
        // only arguments the record has.
        let named =
            |var: DiId, label: bool, expr: ExprId, loc: LocId, args: u64, what: &dyn Display| {
                let fail =
                    |why: &str| Err(IrError::new(InvalidDebugInfo, format!("{what}: {why}")));
                let scope = match (debug.contains(var).then(|| debug.node(var)), label) {
                    (Some(DiNode::LocalVariable(node)), false) => node.scope,
                    (Some(DiNode::Label(node)), true) => node.scope,
                    (_, false) => return fail("its variable is not a DILocalVariable"),
                    (_, true) => return fail("its label is not a DILabel"),
                };
                location(loc, what)?;
                let here = debug.location(loc).and_then(|loc| debug.subprogram_of(loc.scope));
                if scope.and_then(|scope| debug.subprogram_of(scope)) != here {
                    return fail(
                        "its location is in another subprogram than its variable or label",
                    );
                }
                if expr.index() >= debug.expr_count() {
                    return fail("its expression does not exist");
                }
                if debug.ops(expr).any(|op| matches!(op, DiOp::Arg(index) if index >= args)) {
                    return fail("its expression names an argument the record does not have");
                }
                Ok(())
            };
        for var in &body.slot_vars {
            named(
                var.var,
                false,
                var.expr,
                var.loc,
                1,
                &format_args!("the variable of {:?}", var.slot),
            )?;
        }
        for declare in &body.declares {
            let what = format_args!("the declaration of {:?}", declare.var);
            named(declare.var, false, declare.expr, declare.loc, 1, &what)?;
        }
        for (index, record) in body.dbg_records.iter().enumerate() {
            let what = format_args!("debug record {index}");
            let label = record.kind == DbgRecordKind::Label;
            named(record.var, label, record.expr, record.loc, u64::from(record.count), &what)?;
            if record.addr_expr.index() >= debug.expr_count() {
                return Err(IrError::new(
                    InvalidDebugInfo,
                    format!("{what}: its address expression does not exist"),
                ));
            }
        }
        Ok(())
    }

    /// Whether the definition `def` dominates its use by `user`, located
    /// in `at`; `phi` is the user's block when the user is a `phi`.
    #[cold]
    #[inline(never)]
    fn dominated(
        &self,
        dom: &mut Option<Dominators>,
        edges: &mut Vec<u8>,
        def: u32,
        user: u32,
        at: BlockId,
        phi: Option<BlockId>,
    ) -> IrResult<()> {
        let body = self.body;
        let dom = dom.get_or_insert_with(|| Dominators::new(body));
        if !dom.reachable(at) {
            return Ok(());
        }
        if def == user && phi.is_none() {
            let what = "only a phi may use its own result in a block control reaches";
            return self.fail(NotDominated, user, what);
        }
        let fail = || self.fail(NotDominated, user, format!("%{def} does not dominate this use"));
        // The block of `def`: the last block laid out from before it.
        let after = body.layout.partition_point(|&b| body.blocks[b.index()].first <= def);
        let home = body.layout[after - 1];
        if !dom.reachable(home) {
            return fail();
        }
        let d = &body.insts[def as usize];
        if d.op == Opcode::Invoke {
            let normal = BlockId::new(body.calls[d.ops[2].bits() as usize].normal);
            if phi == Some(normal) && at == home {
                return Ok(());
            }
            if !dom.dominates(normal, at) {
                return fail();
            }
            // Whether the edge dominates what its end dominates: the end
            // is entered by that edge alone, or once by it and otherwise
            // only from blocks it dominates. Once per invoke.
            if edges.is_empty() {
                edges.resize(body.blocks.len(), 0);
            }
            let state = &mut edges[home.index()];
            if *state == 0 {
                let preds = dom.preds(normal);
                let mut from_home = 0;
                let ok = preds.len() == 1
                    || preds.iter().all(|&pred| {
                        let pred = BlockId::new(pred);
                        if pred == home {
                            from_home += 1;
                            from_home == 1
                        } else {
                            dom.dominates(normal, pred)
                        }
                    });
                *state = if ok { 1 } else { 2 };
            }
            return if *state == 1 { Ok(()) } else { fail() };
        }
        if home == at {
            // Only a phi's use reaches here for a definition in its own
            // block that is not earlier, and the end of a block is after
            // all of it.
            return if phi.is_some() { Ok(()) } else { fail() };
        }
        if dom.dominates(home, at) { Ok(()) } else { fail() }
    }

    /// Recomputes the use facts from the instructions and compares (§5.6),
    /// and checks that every definition dominates its uses (§5.8): LLVM's
    /// rule, in which a use in a block control does not reach is always
    /// allowed, a `phi`'s use is at the end of its incoming block, and an
    /// `invoke`'s result is defined on its normal edge, so it dominates a
    /// use when the use is that block's `phi` entry for the edge, or the
    /// normal destination dominates the use and is entered only by that
    /// edge or every other edge into it comes from a block it dominates.
    /// A `callbr` result is an ordinary definition at the end of its
    /// block.
    ///
    /// The dominator tree is built only for a use outside its
    /// definition's block (or one before it in a block), in the same pass.
    fn use_facts(&self) -> IrResult<()> {
        let body = self.body;
        let mut uses = vec![UseFacts::NONE; body.insts.len()];
        let mut arg_uses = vec![UseFacts::NONE; body.arg_uses.len()];
        let entry = body.entry();
        let mut dom: Option<Dominators> = None;
        let mut edges: Vec<u8> = Vec::new();
        for block in body.blocks() {
            let here = block.id();
            let data = body.blocks[here.index()];
            for raw in data.first..data.end {
                let inst = body.inst(InstId::new(raw));
                if let crate::InstKind::Phi(list) = inst.kind() {
                    // A phi's use is at the end of the incoming block and
                    // is never "the next instruction".
                    for (value, from) in list.incoming() {
                        match value.kind() {
                            ValueKind::Inst(def) => {
                                let def = def.raw();
                                let inside = body.blocks[from.index()].contains(def);
                                uses[def as usize].add(!inside, false);
                                // Anything but an invoke in the incoming
                                // block is defined by the end of it.
                                if !inside || body.insts[def as usize].op == Opcode::Invoke {
                                    self.dominated(
                                        &mut dom,
                                        &mut edges,
                                        def,
                                        raw,
                                        from,
                                        Some(here),
                                    )?;
                                }
                            }
                            ValueKind::Arg(index) => {
                                arg_uses[index as usize].add(from != entry, false);
                            }
                            _ => {}
                        }
                    }
                } else {
                    for &value in inst.operands() {
                        match value.kind() {
                            ValueKind::Inst(def) => {
                                let def = def.raw();
                                let inside = data.first <= def && def < data.end;
                                uses[def as usize].add(!inside, inside && raw == def + 1);
                                if !inside || def >= raw {
                                    self.dominated(&mut dom, &mut edges, def, raw, here, None)?;
                                }
                            }
                            ValueKind::Arg(index) => {
                                arg_uses[index as usize].add(here != entry, false);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        // §6.6: a value a declaration names says so; that is not a use.
        for declare in &body.declares {
            match declare.address.kind() {
                ValueKind::Inst(def) => uses[def.index()].declare(),
                ValueKind::Arg(index) => arg_uses[index as usize].declare(),
                _ => {}
            }
        }
        if let Some(index) = (0..uses.len()).find(|&index| uses[index] != body.uses[index]) {
            let what = format!(
                "{:?} records {:?} and its uses give {:?}",
                InstId::new(index as u32),
                body.uses[index],
                uses[index]
            );
            return Err(IrError::new(UseFactsMismatch, what));
        }
        if let Some(index) =
            (0..arg_uses.len()).find(|&index| arg_uses[index] != body.arg_uses[index])
        {
            let what = format!(
                "argument {index} records {:?} and its uses give {:?}",
                body.arg_uses[index], arg_uses[index]
            );
            return Err(IrError::new(UseFactsMismatch, what));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::body::{DbgRecordKind, UseFacts};
    use crate::debug::expr::ExprId;
    use crate::{
        Body, DiId, FunctionBuilder, IrErrorKind, LocId, Module, Opcode, SymId, TargetSpec, TypeId,
        Value,
    };

    /// `i32 f(i32 %0, ptr %1)` whose two blocks hold a declaration of
    /// `%1`, a value record, a label and an assign record; in a module
    /// without debug tables, so that only the body's own rules apply.
    fn body() -> (Module, Body) {
        let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
        let fn_ty = m.types.function(TypeId::I32, &[TypeId::I32, TypeId::PTR], false).unwrap();
        let mut b = FunctionBuilder::new(&m, SymId::new(0), fn_ty).unwrap();
        let (entry, next) = (b.create_block().unwrap(), b.create_block().unwrap());
        b.begin_block(entry).unwrap();
        let (number, pointer) = (b.arg(0).unwrap(), b.arg(1).unwrap());
        let (var, at) = (DiId::new(1), LocId::new(1));
        b.dbg_declare(pointer, var, ExprId::EMPTY, at).unwrap();
        let sum = b.binary(Opcode::Add, TypeId::I32, number, number).unwrap();
        b.dbg_value(sum, var, ExprId::EMPTY, at).unwrap();
        b.br(next).unwrap();
        b.begin_block(next).unwrap();
        let phi = b.phi(TypeId::I32, &[(sum, entry)]).unwrap();
        b.dbg_label(DiId::new(2), at).unwrap();
        b.dbg_assign(phi, var, ExprId::EMPTY, pointer, ExprId::EMPTY, at).unwrap();
        b.ret(Some(phi)).unwrap();
        let body = b.finish().unwrap();
        body.verify(&m).unwrap();
        (m, body)
    }

    /// `spec/ir.md` §6.8: the rules about a body's debug tables that the
    /// builder cannot be made to break, each broken in the tables.
    #[test]
    fn damaged_debug_tables_do_not_verify() {
        let verdict = |damage: &dyn Fn(&mut Body)| {
            let (m, mut body) = body();
            damage(&mut body);
            body.verify(&m).err().map(|error| error.kind)
        };
        let invalid = Some(IrErrorKind::InvalidDebugInfo);
        assert_eq!(verdict(&|_| {}), None);
        // A record stands before an instruction of the body that is not
        // a phi, at or after the record before it.
        assert_eq!(verdict(&|body| body.dbg_records[2].before = 4), invalid);
        assert_eq!(verdict(&|body| body.dbg_records[1].before = 2), invalid, "the phi");
        assert_eq!(verdict(&|body| body.dbg_records[1].before = 0), invalid, "out of order");
        assert_eq!(verdict(&|body| body.dbg_records[2].before = 1), invalid, "out of order");
        // Its values are in the operand pool and exist.
        assert_eq!(verdict(&|body| body.dbg_records[0].vals = body.vals.len() as u32), invalid);
        assert_eq!(verdict(&|body| body.dbg_records[2].vals = body.vals.len() as u32 - 1), invalid);
        assert_eq!(verdict(&|body| body.dbg_records[0].count = u16::MAX), invalid);
        let missing = Value::inst(crate::InstId::new(40)).unwrap();
        assert_eq!(
            verdict(&|body| {
                let at = body.dbg_records[0].vals as usize;
                body.vals[at] = missing;
            }),
            invalid
        );
        assert_eq!(
            verdict(&|body| {
                let at = body.dbg_records[2].vals as usize + 1;
                body.vals[at] = Value::arg(7).unwrap();
            }),
            invalid,
            "the address of the assign"
        );
        // A label has no values; a value record without a list has one.
        assert_eq!(verdict(&|body| body.dbg_records[1].count = 1), invalid);
        assert_eq!(verdict(&|body| body.dbg_records[1].arg_list = true), invalid);
        assert_eq!(verdict(&|body| body.dbg_records[0].count = 0), invalid);
        assert_eq!(verdict(&|body| body.dbg_records[0].count = 2), invalid);
        assert_eq!(
            verdict(&|body| {
                body.dbg_records[0].count = 0;
                body.dbg_records[0].arg_list = true;
            }),
            None,
            "an empty list is a list"
        );
        assert_eq!(verdict(&|body| body.dbg_records[1].kind = DbgRecordKind::Value), invalid);
        // A declaration names a value of the body that is not a slot.
        assert_eq!(verdict(&|body| body.declares[0].address = missing), invalid);
        assert_eq!(
            verdict(&|body| {
                body.slots.push(crate::body::Slot::new(8, 3));
                body.declares[0].address = Value::slot(crate::SlotId::new(0)).unwrap();
            }),
            invalid
        );
        // "Named by a declaration" is a use fact: it is recomputed.
        let facts = Some(IrErrorKind::UseFactsMismatch);
        assert_eq!(verdict(&|body| body.arg_uses[1] = UseFacts::NONE), facts);
        assert_eq!(verdict(&|body| body.arg_uses[0].declare()), facts);
        assert_eq!(verdict(&|body| body.uses[0].declare()), facts);
        assert_eq!(verdict(&|body| body.declares.clear()), facts);
        assert_eq!(verdict(&|body| body.declares[0].address = Value::NONE), facts);
    }
}
