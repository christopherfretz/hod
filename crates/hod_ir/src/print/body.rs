//! A function body as text (`spec/ir.md` §9.5): blocks named `b<id>`,
//! values named `%v<id>`, arguments `%a<n>`, stack slots `%s<n>`.

use std::collections::btree_map::Entry;
use std::fmt::{self, Write};

use super::{DeclKey, Dialect, PrintError, R, Writer, write_name, write_quoted};
use crate::asm::InlineAsm;
use crate::body::{Body, CallSite, Callee, Clause, Inst, InstKind, PtrAdd, SHUFFLE_UNDEF};
use crate::ids::{AsmId, BlockId, TypeId};
use crate::inst::{AtomicOrdering, Opcode, RmwOp, SyncScope, TailKind};
use crate::intrinsic::Intrinsic;
use crate::symbol::Symbol;
use crate::types::{TypeKind, TypeTable};
use crate::value::{Value, ValueKind};

/// The first LLVM major that has an intrinsic under the name and with the
/// signature hod knows.
fn intrinsic_since(intrinsic: Intrinsic<'_>) -> u32 {
    match intrinsic {
        Intrinsic::Clmul | Intrinsic::CoroIsInRamp => 22,
        Intrinsic::MinimumNum | Intrinsic::MaximumNum | Intrinsic::Atan2 => 20,
        _ => 19,
    }
}

/// The first LLVM major that has an `atomicrmw` operation.
fn rmw_since(op: RmwOp) -> u32 {
    match op {
        RmwOp::USubCond | RmwOp::USubSat => 20,
        RmwOp::FMaximum | RmwOp::FMinimum => 21,
        _ => 19,
    }
}

/// Writes the name `dialect` gives `intrinsic` with `overloads`, without
/// the `@`: the base name and one suffix per overload type. The name of an
/// `Other` is written as it is.
///
/// # Errors
/// `intrinsic:<name>` if the dialect does not have it, and
/// `intrinsic-overload:<name>` if a type has no spelling in a name.
pub(super) fn write_intrinsic_name<O: Write + ?Sized>(
    out: &mut O,
    types: &TypeTable,
    dialect: Dialect,
    intrinsic: Intrinsic<'_>,
    overloads: &[TypeId],
) -> R {
    let name = intrinsic.name();
    let unsupported = |what: &str| {
        Err(PrintError::Unsupported { construct: format!("{what}:{name}"), symbol: None })
    };
    if intrinsic_since(intrinsic) > dialect.major() {
        return unsupported("intrinsic");
    }
    out.write_str(name)?;
    if matches!(intrinsic, Intrinsic::Other(_)) {
        return Ok(());
    }
    if intrinsic == Intrinsic::ThreadPointer && dialect.major() < 22 {
        // Overloaded on its result only in later LLVMs, which also take
        // the old name.
        return if overloads == [TypeId::PTR] { Ok(()) } else { unsupported("intrinsic-overload") };
    }
    for &ty in overloads {
        out.write_char('.')?;
        if !write_mangled(out, types, ty)? {
            return unsupported("intrinsic-overload");
        }
    }
    Ok(())
}

/// LLVM's spelling of a type in an intrinsic's name
/// (`Intrinsic::mangled_name` writes the same). False for a type that has
/// none; something has been written then.
fn write_mangled<O: Write + ?Sized>(
    out: &mut O,
    types: &TypeTable,
    ty: TypeId,
) -> Result<bool, fmt::Error> {
    if ty.index() >= types.len() {
        return Ok(false);
    }
    match types.kind(ty) {
        TypeKind::Int(bits) => write!(out, "i{bits}")?,
        TypeKind::Half => out.write_str("f16")?,
        TypeKind::BFloat => out.write_str("bf16")?,
        TypeKind::Float => out.write_str("f32")?,
        TypeKind::Double => out.write_str("f64")?,
        TypeKind::Fp128 => out.write_str("f128")?,
        TypeKind::X86Fp80 => out.write_str("f80")?,
        TypeKind::Ptr { addr_space } => write!(out, "p{addr_space}")?,
        TypeKind::Vector { elem, len } => {
            write!(out, "v{len}")?;
            return write_mangled(out, types, elem);
        }
        TypeKind::ScalableVector { elem, min_len } => {
            write!(out, "nxv{min_len}")?;
            return write_mangled(out, types, elem);
        }
        TypeKind::Array { elem, len } => {
            write!(out, "a{len}")?;
            return write_mangled(out, types, elem);
        }
        TypeKind::Struct { fields, .. } => {
            out.write_str("sl_")?;
            for &field in fields {
                if !write_mangled(out, types, field)? {
                    return Ok(false);
                }
            }
            out.write_char('s')?;
        }
        TypeKind::Function { ret, params, variadic } => {
            out.write_str("f_")?;
            if !write_mangled(out, types, ret)? {
                return Ok(false);
            }
            for &param in params {
                if !write_mangled(out, types, param)? {
                    return Ok(false);
                }
            }
            out.write_str(if variadic { "varargf" } else { "f" })?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// The index path of the field of type `field` at byte `offset` of a value
/// of type `agg`, appended to `path`: the inverse of
/// [`crate::aggregate_field`] (§9.5). False if there is none.
///
/// The search is depth first, a struct's fields in ascending order, and
/// takes the first field that has the type and the offset. Two paths lead
/// to one offset and type only where a field has no size, and then both
/// name the same (empty) value.
fn field_path(
    types: &TypeTable,
    agg: TypeId,
    offset: u64,
    field: TypeId,
    path: &mut Vec<u32>,
) -> bool {
    match types.kind(agg) {
        TypeKind::Struct { fields, .. } => {
            let Some(offsets) = types.field_offsets(agg) else { return false };
            // The fields whose extent contains `offset` are adjacent: the
            // one that ends there, empty ones, and the one that starts or
            // continues there.
            let end = offsets.partition_point(|&at| at <= offset);
            let mut start = end;
            while start > 0 && offset - offsets[start - 1] <= types.size_of(fields[start - 1]) {
                start -= 1;
            }
            for index in start..end {
                let (inner, at) = (fields[index], offsets[index]);
                path.push(index as u32);
                if (at == offset && inner == field)
                    || field_path(types, inner, offset - at, field, path)
                {
                    return true;
                }
                path.pop();
            }
            false
        }
        TypeKind::Array { elem, len } => {
            let stride = types.alloc_size_of(elem);
            // Elements without a size all lie at the array's start.
            let (index, rest) = match offset.checked_div(stride) {
                Some(index) => (index, offset % stride),
                None => (0, offset),
            };
            // An index of `extractvalue` is a 32-bit number.
            let Ok(short) = u32::try_from(index) else { return false };
            if index >= len {
                return false;
            }
            path.push(short);
            if (rest == 0 && elem == field) || field_path(types, elem, rest, field, path) {
                return true;
            }
            path.pop();
            false
        }
        _ => false,
    }
}

/// `value`'s low `bits` bits as a signed number.
fn signed(value: u64, bits: u32) -> i64 {
    let shift = 64 - bits.clamp(1, 64);
    ((value << shift) as i64) >> shift
}

/// The element count of a vector type and whether it is scalable.
type Lanes = Option<(u32, bool)>;

impl<'a, W: Write + ?Sized> Writer<'a, W> {
    /// The blocks of a body, in layout order, between the braces.
    pub(super) fn body(&mut self, body: &'a Body) -> R {
        if body.block_count() == 0 {
            return self.malformed("a body without a block");
        }
        let entry = body.entry();
        for block in body.blocks() {
            writeln!(self.out, "b{}:", block.id().raw())?;
            if block.id() == entry {
                // The stack slots: fixed objects of the frame.
                for (index, slot) in body.slots().iter().enumerate() {
                    let (size, align) = (slot.size(), slot.align());
                    writeln!(self.out, "  %s{index} = alloca [{size} x i8], align {align}")?;
                }
                self.dbg_slot_vars(body)?;
            }
            for inst in block.insts() {
                self.s("  ")?;
                self.inst(body, inst)?;
                self.dbg_inst(body, inst)?;
                self.s("\n")?;
            }
        }
        Ok(())
    }

    // ---- DEBUG INFORMATION HOOKS ----
    //
    // The printer writes no debug information yet (§9.7). These three are
    // where it goes: ` !dbg !N` on the `define` line (the subprogram),
    // `#dbg_declare` records (dialect 19: `llvm.dbg.declare` calls) for
    // the slot variables after the slots, and ` , !dbg !N` after an
    // instruction, with the metadata nodes written by `Writer::metadata`.

    #[expect(clippy::unused_self, clippy::unnecessary_wraps, reason = "the debug-info hook")]
    pub(super) fn dbg_function(&mut self, _symbol: Symbol<'_>) -> R {
        Ok(())
    }

    #[expect(clippy::unused_self, clippy::unnecessary_wraps, reason = "the debug-info hook")]
    fn dbg_slot_vars(&mut self, _body: &Body) -> R {
        Ok(())
    }

    #[expect(clippy::unused_self, clippy::unnecessary_wraps, reason = "the debug-info hook")]
    fn dbg_inst(&mut self, _body: &Body, _inst: Inst<'_>) -> R {
        Ok(())
    }

    // ---- operands ----

    fn value_ty(&self, body: &Body, value: Value) -> Result<TypeId, PrintError> {
        if let ValueKind::Const(id) = value.kind() {
            return self.const_ty(id);
        }
        Ok(body.value_type(self.m, value))
    }

    /// An operand, without its type.
    fn value(&mut self, body: &Body, value: Value) -> R {
        match value.kind() {
            ValueKind::Inst(id) if id.index() < body.inst_count() => {
                write!(self.out, "%v{}", id.raw())?;
            }
            ValueKind::Arg(index) if index < body.arg_count() => write!(self.out, "%a{index}")?,
            ValueKind::Slot(id) if id.index() < body.slots().len() => {
                write!(self.out, "%s{}", id.raw())?;
            }
            ValueKind::Const(id) => self.constant(id)?,
            ValueKind::Sym(id) => self.sym_ref(id)?,
            ValueKind::Asm(_) => {
                return self.malformed("inline assembly that is not the callee of a call");
            }
            ValueKind::None => return self.malformed("an operand that is no value"),
            other => return self.malformed(format!("{other:?} is not a value of the body")),
        }
        Ok(())
    }

    /// `type operand`.
    fn typed(&mut self, body: &Body, value: Value) -> R {
        let ty = self.value_ty(body, value)?;
        self.ty(ty)?;
        self.s(" ")?;
        self.value(body, value)
    }

    /// `label %b<id>`.
    fn label(&mut self, body: &Body, block: BlockId) -> R {
        if block.index() >= body.block_count() {
            return self.malformed(format!("{block:?} is not a block of the body"));
        }
        write!(self.out, "label %b{}", block.raw())?;
        Ok(())
    }

    /// `%v<id> = `.
    fn def(&mut self, id: u32) -> R {
        write!(self.out, "%v{id} = ")?;
        Ok(())
    }

    /// ` syncscope("singlethread") ordering`.
    fn atomic(&mut self, scope: SyncScope, ordering: AtomicOrdering) -> R {
        if scope == SyncScope::SingleThread {
            self.s(" syncscope(\"singlethread\")")?;
        }
        self.s(" ")?;
        self.s(ordering.name())
    }

    // ---- instructions ----

    #[expect(clippy::too_many_lines, reason = "one arm per instruction kind")]
    fn inst(&mut self, body: &'a Body, inst: Inst<'a>) -> R {
        let id = inst.id().raw();
        let (ty, op) = (inst.ty(), inst.opcode());
        match inst.kind() {
            InstKind::Binary { lhs, rhs } => {
                self.def(id)?;
                self.s(op.name())?;
                self.s(" ")?;
                self.ty(ty)?;
                self.s(" ")?;
                self.value(body, lhs)?;
                self.s(", ")?;
                self.value(body, rhs)
            }
            InstKind::Unary { arg } => {
                self.def(id)?;
                self.s(op.name())?;
                self.s(" ")?;
                self.typed(body, arg)
            }
            InstKind::Cast { arg } => {
                let mut name = op.name();
                if op == Opcode::PtrToAddr && self.dialect.major() < 22 {
                    let from = self.value_ty(body, arg)?;
                    if !self.address_is_whole_pointer(from) {
                        return self.unsupported("inst:ptrtoaddr");
                    }
                    name = "ptrtoint";
                }
                self.def(id)?;
                self.s(name)?;
                self.s(" ")?;
                self.typed(body, arg)?;
                self.s(" to ")?;
                self.ty(ty)
            }
            InstKind::ICmp { pred, lhs, rhs } => self.compare(body, id, "icmp ", pred.name(), lhs, rhs),
            InstKind::FCmp { pred, lhs, rhs } => self.compare(body, id, "fcmp ", pred.name(), lhs, rhs),
            InstKind::Load { ptr, mem } => {
                self.def(id)?;
                self.s("load ")?;
                self.s(if mem.ordering().is_some() { "atomic " } else { "" })?;
                self.s(if mem.is_volatile() { "volatile " } else { "" })?;
                self.ty(ty)?;
                self.s(", ")?;
                self.typed(body, ptr)?;
                if let Some(ordering) = mem.ordering() {
                    self.atomic(mem.scope(), ordering)?;
                }
                Ok(write!(self.out, ", align {}", mem.align())?)
            }
            InstKind::Store { value, ptr, mem } => {
                self.s("store ")?;
                self.s(if mem.ordering().is_some() { "atomic " } else { "" })?;
                self.s(if mem.is_volatile() { "volatile " } else { "" })?;
                self.typed(body, value)?;
                self.s(", ")?;
                self.typed(body, ptr)?;
                if let Some(ordering) = mem.ordering() {
                    self.atomic(mem.scope(), ordering)?;
                }
                Ok(write!(self.out, ", align {}", mem.align())?)
            }
            InstKind::Alloca { count, elem_size, align } => {
                self.def(id)?;
                write!(self.out, "alloca [{elem_size} x i8], ")?;
                self.typed(body, count)?;
                Ok(write!(self.out, ", align {align}")?)
            }
            InstKind::Fence { ordering, scope } => {
                self.s("fence")?;
                self.atomic(scope, ordering)
            }
            InstKind::CmpXchg { ptr, expected, replacement, mem, failure, weak } => {
                let Some(success) = mem.ordering() else {
                    return self.malformed("a cmpxchg without an ordering");
                };
                self.def(id)?;
                self.s("cmpxchg ")?;
                self.s(if weak { "weak " } else { "" })?;
                self.s(if mem.is_volatile() { "volatile " } else { "" })?;
                self.typed(body, ptr)?;
                self.s(", ")?;
                self.typed(body, expected)?;
                self.s(", ")?;
                self.typed(body, replacement)?;
                self.atomic(mem.scope(), success)?;
                Ok(write!(self.out, " {}, align {}", failure.name(), mem.align())?)
            }
            InstKind::AtomicRmw { op: rmw, ptr, value, mem } => {
                let Some(ordering) = mem.ordering() else {
                    return self.malformed("an atomicrmw without an ordering");
                };
                if rmw_since(rmw) > self.dialect.major() {
                    return self.unsupported(format!("atomicrmw:{}", rmw.name()));
                }
                self.def(id)?;
                self.s("atomicrmw ")?;
                self.s(if mem.is_volatile() { "volatile " } else { "" })?;
                self.s(rmw.name())?;
                self.s(" ")?;
                self.typed(body, ptr)?;
                self.s(", ")?;
                self.typed(body, value)?;
                self.atomic(mem.scope(), ordering)?;
                Ok(write!(self.out, ", align {}", mem.align())?)
            }
            InstKind::PtrAdd(ptr_add) => self.ptr_add(body, id, &ptr_add),
            InstKind::ExtractValue { agg, offset } => {
                let agg_ty = self.value_ty(body, agg)?;
                self.def(id)?;
                self.s("extractvalue ")?;
                self.typed(body, agg)?;
                self.field_indices(agg_ty, offset, ty)
            }
            InstKind::InsertValue { agg, value, offset } => {
                let (agg_ty, field) = (self.value_ty(body, agg)?, self.value_ty(body, value)?);
                self.def(id)?;
                self.s("insertvalue ")?;
                self.typed(body, agg)?;
                self.s(", ")?;
                self.typed(body, value)?;
                self.field_indices(agg_ty, offset, field)
            }
            InstKind::ExtractElement { vector, index } => {
                self.def(id)?;
                self.s("extractelement ")?;
                self.typed(body, vector)?;
                self.s(", ")?;
                self.typed(body, index)
            }
            InstKind::InsertElement { vector, value, index } => {
                self.def(id)?;
                self.s("insertelement ")?;
                self.typed(body, vector)?;
                self.s(", ")?;
                self.typed(body, value)?;
                self.s(", ")?;
                self.typed(body, index)
            }
            InstKind::ShuffleVector { first, second, mask } => {
                self.def(id)?;
                self.s("shufflevector ")?;
                self.typed(body, first)?;
                self.s(", ")?;
                self.typed(body, second)?;
                self.s(", ")?;
                self.shuffle_mask(ty, mask)
            }
            InstKind::Phi(phi) => {
                self.def(id)?;
                if phi.is_empty() {
                    // The block has no predecessor, and LLVM has no `phi`
                    // without an incoming value. Nothing reads this one.
                    self.s("freeze ")?;
                    self.ty(ty)?;
                    return self.s(" poison");
                }
                self.s("phi ")?;
                self.ty(ty)?;
                for (index, (value, block)) in phi.incoming().enumerate() {
                    if block.index() >= body.block_count() {
                        return self.malformed(format!("{block:?} is not a block of the body"));
                    }
                    self.s(if index == 0 { " [ " } else { ", [ " })?;
                    self.value(body, value)?;
                    write!(self.out, ", %b{} ]", block.raw())?;
                }
                Ok(())
            }
            InstKind::Select { cond, if_true, if_false } => {
                self.def(id)?;
                self.s("select ")?;
                self.typed(body, cond)?;
                self.s(", ")?;
                self.typed(body, if_true)?;
                self.s(", ")?;
                self.typed(body, if_false)
            }
            InstKind::VaArg { list } => {
                self.def(id)?;
                self.s("va_arg ")?;
                self.typed(body, list)?;
                self.s(", ")?;
                self.ty(ty)
            }
            InstKind::Call(call) => self.call(body, id, ty, call),
            InstKind::LandingPad(pad) => {
                self.def(id)?;
                self.s("landingpad ")?;
                self.ty(ty)?;
                if pad.is_cleanup() {
                    self.s(" cleanup")?;
                }
                for clause in pad.clauses() {
                    let (keyword, constant) = match *clause {
                        Clause::Catch(constant) => (" catch ", constant),
                        Clause::Filter(constant) => (" filter ", constant),
                    };
                    self.s(keyword)?;
                    self.typed_const(constant)?;
                }
                Ok(())
            }
            InstKind::Br { target } => {
                self.s("br ")?;
                self.label(body, target)
            }
            InstKind::CondBr { cond, if_true, if_false } => {
                self.s("br ")?;
                self.typed(body, cond)?;
                self.s(", ")?;
                self.label(body, if_true)?;
                self.s(", ")?;
                self.label(body, if_false)
            }
            InstKind::Switch(switch) => {
                self.s("switch ")?;
                self.typed(body, switch.cond)?;
                self.s(", ")?;
                self.label(body, switch.default)?;
                self.s(" [")?;
                for (value, block) in switch.cases() {
                    self.s(" ")?;
                    self.typed_const(value)?;
                    self.s(", ")?;
                    self.label(body, block)?;
                }
                self.s(" ]")
            }
            InstKind::IndirectBr { addr, targets } => {
                self.s("indirectbr ")?;
                self.typed(body, addr)?;
                self.s(", [")?;
                for (index, block) in targets.enumerate() {
                    if index > 0 {
                        self.s(", ")?;
                    }
                    self.label(body, block)?;
                }
                self.s("]")
            }
            InstKind::Ret { value: None } => self.s("ret void"),
            InstKind::Ret { value: Some(value) } => {
                self.s("ret ")?;
                self.typed(body, value)
            }
            InstKind::Resume { value } => {
                self.s("resume ")?;
                self.typed(body, value)
            }
            InstKind::Unreachable => self.s("unreachable"),
            // The operands of a funclet instruction are not in the IR.
            InstKind::Funclet => self.unsupported(format!("inst:{}", op.name())),
        }
    }

    fn compare(
        &mut self,
        body: &Body,
        id: u32,
        keyword: &str,
        pred: &str,
        lhs: Value,
        rhs: Value,
    ) -> R {
        self.def(id)?;
        self.s(keyword)?;
        self.s(pred)?;
        self.s(" ")?;
        self.typed(body, lhs)?;
        self.s(", ")?;
        self.value(body, rhs)
    }

    /// `, i, j`: the index path of the field of type `field` at byte
    /// `offset` of a value of type `agg`.
    fn field_indices(&mut self, agg: TypeId, offset: u64, field: TypeId) -> R {
        let types = &self.m.types;
        let mut path = std::mem::take(&mut self.path);
        path.clear();
        let found = field.index() < types.len() && field_path(types, agg, offset, field, &mut path);
        let mut result = Ok(());
        for &index in &path {
            result = result.and_then(|()| write!(self.out, ", {index}"));
        }
        // The path's storage serves the next instruction.
        self.path = path;
        if !found {
            return self.malformed(format!("{agg:?} has no field of {field:?} at byte {offset}"));
        }
        Ok(result?)
    }

    /// The mask of a `shufflevector` whose result has type `ty`.
    fn shuffle_mask(&mut self, ty: TypeId, mask: &[u32]) -> R {
        if let TypeKind::ScalableVector { min_len, .. } = self.m.types.kind(ty) {
            // LLVM has two masks for a scalable vector.
            let all = |wanted: u32| mask.iter().all(|&entry| entry == wanted);
            let text = match () {
                () if all(0) => "zeroinitializer",
                () if all(SHUFFLE_UNDEF) => "poison",
                () => return self.unsupported("shufflevector:scalable-mask"),
            };
            return Ok(write!(self.out, "<vscale x {min_len} x i32> {text}")?);
        }
        write!(self.out, "<{} x i32> <", mask.len())?;
        for (index, &entry) in mask.iter().enumerate() {
            self.s(if index == 0 { "i32 " } else { ", i32 " })?;
            if entry == SHUFFLE_UNDEF {
                self.s("poison")?;
            } else {
                write!(self.out, "{entry}")?;
            }
        }
        self.s(">")
    }

    // ---- address arithmetic ----

    /// `<N x ` or `<vscale x N x `, before the element type of a vector.
    fn lanes_open(&mut self, lanes: Lanes) -> R {
        match lanes {
            None => {}
            Some((len, false)) => write!(self.out, "<{len} x ")?,
            Some((len, true)) => write!(self.out, "<vscale x {len} x ")?,
        }
        Ok(())
    }

    /// `i<bits>`, or a vector of them.
    fn int_lanes(&mut self, lanes: Lanes, bits: u32) -> R {
        self.lanes_open(lanes)?;
        write!(self.out, "i{bits}")?;
        self.s(if lanes.is_some() { ">" } else { "" })
    }

    /// A `PtrAdd` as `getelementptr i8` steps (§9.5): one for the constant
    /// offset, then one per index, each index first brought to the
    /// pointer's index width and multiplied by its scale. The last step
    /// has the instruction's name; the ones before it `.g<n>`, and the
    /// converted and scaled index `k` `.c<k>` and `.m<k>`.
    #[expect(clippy::too_many_lines, reason = "one walk over the steps")]
    fn ptr_add(&mut self, body: &'a Body, id: u32, ptr_add: &PtrAdd<'a>) -> R {
        let m = self.m;
        let types = &m.types;
        let base_ty = self.value_ty(body, ptr_add.base)?;
        let (pointer, mut lanes): (TypeId, Lanes) = match types.kind(base_ty) {
            TypeKind::Vector { elem, len } => (elem, Some((len, false))),
            TypeKind::ScalableVector { elem, min_len } => (elem, Some((min_len, true))),
            _ => (base_ty, None),
        };
        let TypeKind::Ptr { addr_space } = types.kind(pointer) else {
            return self.malformed("a ptradd whose base is not a pointer");
        };
        let width = types.data_layout().pointer(addr_space).index_bits;
        if !(1..=64).contains(&width) {
            return self.unsupported("ptradd:index-width");
        }
        let count = ptr_add.index_count();
        let steps = count + usize::from(ptr_add.offset != 0 || count == 0);
        let mut step = 0;
        // `getelementptr i8, <pointer type> <the pointer so far>`, on a
        // line of its own after the first.
        let gep = |writer: &mut Self, step: usize, lanes: Lanes, first: bool| -> R {
            if !first {
                writer.s("\n  ")?;
            }
            if step + 1 == steps {
                write!(writer.out, "%v{id} = getelementptr i8, ")?;
            } else {
                write!(writer.out, "%v{id}.g{step} = getelementptr i8, ")?;
            }
            writer.lanes_open(lanes)?;
            writer.ty(pointer)?;
            writer.s(if lanes.is_some() { "> " } else { " " })?;
            if step == 0 {
                writer.value(body, ptr_add.base)?;
            } else {
                write!(writer.out, "%v{id}.g{}", step - 1)?;
            }
            writer.s(", ")
        };
        let mut first = true;
        if steps > count {
            gep(self, step, lanes, first)?;
            write!(self.out, "i{width} {}", signed(ptr_add.offset as u64, width))?;
            first = false;
            step += 1;
        }
        for (k, (index, scale)) in ptr_add.indices().enumerate() {
            let index_ty = self.value_ty(body, index)?;
            let (bits, index_lanes): (Option<u32>, Lanes) = match types.kind(index_ty) {
                TypeKind::Vector { elem, len } => (types.int_bits(elem), Some((len, false))),
                TypeKind::ScalableVector { elem, min_len } => {
                    (types.int_bits(elem), Some((min_len, true)))
                }
                _ => (types.int_bits(index_ty), None),
            };
            let Some(bits) = bits else {
                return self.malformed("a ptradd index that is not an integer");
            };
            // The newest name of the index: itself, `.c<k>` or `.m<k>`.
            let mut suffix = "";
            if bits != width {
                if !first {
                    self.s("\n  ")?;
                }
                first = false;
                let cast = if bits < width { "sext" } else { "trunc" };
                write!(self.out, "%v{id}.c{k} = {cast} ")?;
                self.typed(body, index)?;
                self.s(" to ")?;
                self.int_lanes(index_lanes, width)?;
                suffix = "c";
            }
            if scale != 1 {
                if !first {
                    self.s("\n  ")?;
                }
                first = false;
                write!(self.out, "%v{id}.m{k} = mul ")?;
                self.int_lanes(index_lanes, width)?;
                self.s(" ")?;
                if suffix.is_empty() {
                    self.value(body, index)?;
                } else {
                    write!(self.out, "%v{id}.{suffix}{k}")?;
                }
                let scale = signed(scale, width);
                if index_lanes.is_some() {
                    write!(self.out, ", splat (i{width} {scale})")?;
                } else {
                    write!(self.out, ", {scale}")?;
                }
                suffix = "m";
            }
            gep(self, step, lanes, first)?;
            first = false;
            self.int_lanes(index_lanes, width)?;
            self.s(" ")?;
            if suffix.is_empty() {
                self.value(body, index)?;
            } else {
                write!(self.out, "%v{id}.{suffix}{k}")?;
            }
            // A vector index makes every later pointer a vector.
            lanes = lanes.or(index_lanes);
            step += 1;
        }
        Ok(())
    }

    // ---- calls ----

    fn call(&mut self, body: &'a Body, id: u32, ty: TypeId, call: CallSite<'a>) -> R {
        let m = self.m;
        let fn_ty = call.fn_type();
        if fn_ty.index() >= m.types.len() {
            return self.malformed(format!("{fn_ty:?} is not a type of the module"));
        }
        let TypeKind::Function { ret, variadic, .. } = m.types.kind(fn_ty) else {
            return self.malformed("a call whose type is not a function type");
        };
        if ty != TypeId::VOID {
            self.def(id)?;
        }
        let opcode = call.opcode();
        match (opcode, call.tail()) {
            (Opcode::Call, TailKind::None) => self.s("call ")?,
            (Opcode::Call, tail) => {
                self.s(tail.name())?;
                self.s(" call ")?;
            }
            (Opcode::Invoke, TailKind::None) => self.s("invoke ")?,
            (Opcode::CallBr, TailKind::None) => self.s("callbr ")?,
            // LLVM has the markers on `call` only.
            (_, tail) => return self.unsupported(format!("{}:{}", opcode.name(), tail.name())),
        }
        self.call_conv(call.cc())?;
        self.param_attrs(call.ret_attrs(), false)?;
        // LLVM wants the whole function type where the arguments do not
        // give it.
        self.ty(if variadic { fn_ty } else { ret })?;
        self.s(" ")?;
        match call.callee() {
            Callee::Intrinsic(intrinsic) => self.intrinsic_callee(call, intrinsic)?,
            Callee::Value(callee) => match callee.kind() {
                ValueKind::Asm(asm) => self.asm(asm)?,
                _ => self.value(body, callee)?,
            },
        }
        self.s("(")?;
        for (index, &arg) in call.args().iter().enumerate() {
            if index > 0 {
                self.s(", ")?;
            }
            let arg_ty = self.value_ty(body, arg)?;
            self.ty(arg_ty)?;
            self.param_attrs(call.arg_attrs(index), true)?;
            self.s(" ")?;
            self.value(body, arg)?;
        }
        if call.tail() == TailKind::MustTail
            && matches!(m.types.kind(body.fn_type()), TypeKind::Function { variadic: true, .. })
        {
            // A `musttail` call in a variadic function passes the
            // function's variable arguments on, and LLVM's text says so.
            self.s(if call.args().is_empty() { "..." } else { ", ..." })?;
        }
        self.s(")")?;
        self.s(if call.is_noreturn() { " noreturn" } else { "" })?;
        self.s(if call.is_nounwind() { " nounwind" } else { "" })?;
        self.s(if call.returns_twice() { " returns_twice" } else { "" })?;
        if call.bundle_count() > 0 {
            self.s(" [ ")?;
            for (index, bundle) in call.bundles().enumerate() {
                if index > 0 {
                    self.s(", ")?;
                }
                write_quoted(self.out, bundle.tag.as_bytes())?;
                self.s("(")?;
                for (at, &input) in bundle.inputs.iter().enumerate() {
                    if at > 0 {
                        self.s(", ")?;
                    }
                    self.typed(body, input)?;
                }
                self.s(")")?;
            }
            self.s(" ]")?;
        }
        if let Some(normal) = call.normal_dest() {
            self.s("\n          to ")?;
            self.label(body, normal)?;
        }
        if let Some(unwind) = call.unwind_dest() {
            self.s(" unwind ")?;
            self.label(body, unwind)?;
        }
        if opcode == Opcode::CallBr {
            self.s(" [")?;
            for (index, block) in call.indirect_dests().enumerate() {
                if index > 0 {
                    self.s(", ")?;
                }
                self.label(body, block)?;
            }
            self.s("]")?;
        }
        Ok(())
    }

    /// `asm sideeffect "template", "constraints"`.
    fn asm(&mut self, id: AsmId) -> R {
        if id.index() >= self.m.asm_count() {
            return self.malformed(format!("{id:?} is not inline assembly of the module"));
        }
        const FLAGS: [(u8, &str); 4] = [
            (InlineAsm::SIDE_EFFECT, "sideeffect "),
            (InlineAsm::ALIGN_STACK, "alignstack "),
            (InlineAsm::INTEL_DIALECT, "inteldialect "),
            (InlineAsm::UNWIND, "unwind "),
        ];
        let asm = self.m.asm(id);
        self.s("asm ")?;
        for (flag, keyword) in FLAGS {
            if asm.has(flag) {
                self.s(keyword)?;
            }
        }
        write_quoted(self.out, asm.template)?;
        self.s(", ")?;
        write_quoted(self.out, asm.constraints)
    }

    /// The name of an intrinsic callee; the declaration it needs is noted
    /// for the end of the module.
    fn intrinsic_callee(&mut self, call: CallSite<'a>, intrinsic: Intrinsic<'a>) -> R {
        let overloads = call.overloads();
        let cur = self.cur;
        let at = |error| match error {
            PrintError::Unsupported { construct, .. } => {
                PrintError::Unsupported { construct, symbol: cur }
            }
            other => other,
        };
        let mut key = DeclKey { code: intrinsic.code(), name: "", overloads: [TypeId::VOID; 3] };
        if let Intrinsic::Other(name) = intrinsic {
            write_name(self.out, '@', name.as_bytes())?;
            key.name = name;
        } else {
            self.s("@")?;
            write_intrinsic_name(self.out, &self.m.types, self.dialect, intrinsic, overloads)
                .map_err(at)?;
            let Some(slots) = key.overloads.get_mut(..overloads.len()) else {
                return self.malformed("an intrinsic with more than three overload types");
            };
            slots.copy_from_slice(overloads);
        }
        if self.alone {
            return Ok(());
        }
        match self.decls.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(call.fn_type());
            }
            Entry::Occupied(entry) if *entry.get() != call.fn_type() => {
                // One name declared with two types is not a module.
                return self.unsupported(format!("intrinsic-signature:{}", intrinsic.name()));
            }
            Entry::Occupied(_) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inst::aggregate_field;
    use crate::module::Module;
    use crate::target::TargetSpec;

    /// §9.5: the name the printer writes for an intrinsic is the one
    /// `Intrinsic::mangled_name` builds, for every kind of overload type.
    #[test]
    fn intrinsic_names_match_mangled_name() {
        let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
        let t = &mut m.types;
        let i7 = t.int(7).unwrap();
        let p3 = t.ptr(3).unwrap();
        let vec = t.vector(TypeId::FLOAT, 4).unwrap();
        let scalable = t.scalable_vector(TypeId::I8, 16).unwrap();
        let array = t.array(TypeId::BFLOAT, 3).unwrap();
        let record = t.struct_type(&[TypeId::I32, vec, TypeId::X86_FP80], true).unwrap();
        let function = t.function(TypeId::VOID, &[TypeId::PTR, TypeId::FP128], true).unwrap();
        let opaque = t.opaque_struct().unwrap();
        let has_name = [
            i7,
            p3,
            vec,
            scalable,
            array,
            record,
            TypeId::HALF,
            TypeId::DOUBLE,
            TypeId::I1,
            TypeId::PTR,
        ];
        for ty in has_name {
            let overloads = [ty, TypeId::I64, ty];
            let mut text = String::new();
            let dialect = Dialect::LLVM22;
            write_intrinsic_name(&mut text, &m.types, dialect, Intrinsic::Memcpy, &overloads)
                .unwrap();
            assert_eq!(Some(text), Intrinsic::Memcpy.mangled_name(&m.types, &overloads));
        }
        for ty in [function, opaque, TypeId::VOID, TypeId::TOKEN] {
            let mut text = String::new();
            let written =
                write_intrinsic_name(&mut text, &m.types, Dialect::LLVM22, Intrinsic::Abs, &[ty]);
            let expected = Intrinsic::Abs.mangled_name(&m.types, &[ty]);
            match expected {
                Some(expected) => assert_eq!((written, text), (Ok(()), expected)),
                None => assert!(matches!(written, Err(PrintError::Unsupported { construct, .. })
                    if construct == "intrinsic-overload:llvm.abs")),
            }
        }
    }

    /// §9.2: `llvm.thread.pointer` has a suffix only in dialect 22, and an
    /// intrinsic a dialect lacks is an error that names it.
    #[test]
    fn intrinsic_names_follow_the_dialect() {
        let m = Module::new(TargetSpec::aarch64_linux_gnu());
        let name = |dialect, intrinsic: Intrinsic<'_>, overloads: &[TypeId]| {
            let mut text = String::new();
            write_intrinsic_name(&mut text, &m.types, dialect, intrinsic, overloads).map(|()| text)
        };
        let pointer = [TypeId::PTR];
        assert_eq!(
            name(Dialect::LLVM22, Intrinsic::ThreadPointer, &pointer).unwrap(),
            "llvm.thread.pointer.p0"
        );
        for dialect in [Dialect::LLVM19, Dialect::LLVM20, Dialect::LLVM21] {
            assert_eq!(
                name(dialect, Intrinsic::ThreadPointer, &pointer).unwrap(),
                "llvm.thread.pointer"
            );
        }
        let refused = |dialect, intrinsic: Intrinsic<'_>, overloads: &[TypeId]| {
            match name(dialect, intrinsic, overloads) {
                Err(PrintError::Unsupported { construct, symbol: None }) => construct,
                other => panic!("{other:?}"),
            }
        };
        let int = [TypeId::I32];
        let float = [TypeId::DOUBLE];
        assert_eq!(refused(Dialect::LLVM21, Intrinsic::Clmul, &int), "intrinsic:llvm.clmul");
        assert_eq!(refused(Dialect::LLVM19, Intrinsic::Atan2, &float), "intrinsic:llvm.atan2");
        assert_eq!(
            refused(Dialect::LLVM19, Intrinsic::MinimumNum, &float),
            "intrinsic:llvm.minimumnum"
        );
        assert_eq!(name(Dialect::LLVM20, Intrinsic::Atan2, &float).unwrap(), "llvm.atan2.f64");
        assert_eq!(name(Dialect::LLVM22, Intrinsic::Clmul, &int).unwrap(), "llvm.clmul.i32");
        assert_eq!(name(Dialect::LLVM19, Intrinsic::Other("llvm.x.y"), &[]).unwrap(), "llvm.x.y");
    }

    /// §9.5: the index path is the inverse of `aggregate_field`: every
    /// field of a nest of structs and arrays is found again from its
    /// offset and type, at a path with the same offset and type.
    #[test]
    fn field_paths_invert_aggregate_field() {
        // Every path of `ty`, depth first.
        fn paths(types: &TypeTable, ty: TypeId, path: &mut Vec<u32>, out: &mut Vec<Vec<u32>>) {
            let (count, child): (u32, Box<dyn Fn(u32) -> TypeId>) = match types.kind(ty) {
                TypeKind::Struct { fields, .. } => {
                    let fields = fields.to_vec();
                    (fields.len() as u32, Box::new(move |index| fields[index as usize]))
                }
                TypeKind::Array { elem, len } => (len as u32, Box::new(move |_| elem)),
                _ => return,
            };
            for index in 0..count {
                path.push(index);
                out.push(path.clone());
                paths(types, child(index), path, out);
                path.pop();
            }
        }

        let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
        let t = &mut m.types;
        let empty = t.struct_type(&[], false).unwrap();
        let no_elems = t.array(TypeId::I64, 0).unwrap();
        let pair = t.struct_type(&[TypeId::I32, TypeId::I64], false).unwrap();
        let packed = t.struct_type(&[TypeId::I8, TypeId::I32, TypeId::I8], true).unwrap();
        let pairs = t.array(pair, 3).unwrap();
        let bytes = t.array(TypeId::I8, 5).unwrap();
        let vec = t.vector(TypeId::I32, 4).unwrap();
        let inner = t.struct_type(&[TypeId::I8, empty, pairs, no_elems, packed], false).unwrap();
        let outer =
            t.struct_type(&[inner, empty, bytes, vec, TypeId::PTR, pair, empty], false).unwrap();
        let grid = t.array(outer, 2).unwrap();
        let empties = t.array(empty, 4).unwrap();
        let holder = t.struct_type(&[empties, TypeId::I32, empties], false).unwrap();

        let mut checked = 0;
        for agg in [pair, packed, pairs, inner, outer, grid, holder] {
            let mut all = Vec::new();
            paths(&m.types, agg, &mut Vec::new(), &mut all);
            for path in all {
                let (offset, field) = aggregate_field(&m.types, agg, &path).unwrap();
                let mut found = Vec::new();
                assert!(
                    field_path(&m.types, agg, offset, field, &mut found),
                    "{agg:?} {path:?}: no path to {field:?} at {offset}"
                );
                assert_eq!(
                    aggregate_field(&m.types, agg, &found).unwrap(),
                    (offset, field),
                    "{agg:?} {path:?} found {found:?}"
                );
                // Only a field without a size can be reached two ways.
                if m.types.size_of(field) != 0 {
                    assert_eq!(found, path, "{agg:?}");
                }
                checked += 1;
            }
        }
        assert!(checked > 100, "{checked}");

        // What is not there is not found, and nothing is left in the path.
        let mut path = Vec::new();
        assert!(!field_path(&m.types, pair, 4, TypeId::I32, &mut path));
        assert!(!field_path(&m.types, pair, 8, TypeId::I32, &mut path));
        assert!(!field_path(&m.types, pair, 16, TypeId::I64, &mut path));
        assert!(!field_path(&m.types, pairs, 48, TypeId::I32, &mut path));
        assert!(!field_path(&m.types, vec, 0, TypeId::I32, &mut path));
        assert!(!field_path(&m.types, TypeId::I32, 0, TypeId::I32, &mut path));
        assert!(path.is_empty());
    }

    /// §9.5: constants of an index width are written as signed numbers.
    #[test]
    fn index_constants_are_signed() {
        assert_eq!(signed(u64::MAX, 64), -1);
        assert_eq!(signed(0xffff_fff8, 32), -8);
        assert_eq!(signed(0x1_0000_0003, 32), 3);
        assert_eq!(signed(24, 64), 24);
    }
}
