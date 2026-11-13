//! `A64`, the AArch64 Linux implementation of `hod_cg::Target`
//! (`spec/codegen.md` §5.5, §8.2): the register description, the value
//! classes, the glue, and the one dispatch of every instruction to the
//! module of its group. Owned by WP-033.
//!
//! # Dispatch
//!
//! [`Target::lower`] is one `match` and is final: a group package fills
//! in its module's entry points and does not edit it. Where an opcode
//! serves several groups, the class of the value decides (§2.2):
//!
//! | instruction | `Gpr` | `Fpr` | `Mem` |
//! |---|---|---|---|
//! | binary, `freeze`, `select` | `int` | `float` | `aggr` |
//! | cast | `int` (both sides `Gpr`) | `float` (either side `Fpr`) | `aggr` (either side `Mem`) |
//! | `icmp` | `int` | (vectors: `float`) | `aggr` (the operands) |
//! | `fcmp`, `fneg` | | `float` | |
//! | `load`, `store` (the value's class) | `mem` | `mem` | `aggr` |
//!
//! `alloca`, `ptradd`, `llvm.stacksave`, `llvm.stackrestore` are `mem`;
//! `extractvalue`, `insertvalue` and the `*.with.overflow` intrinsics are
//! `aggr`; `condbr`, `switch`, `indirectbr` are `control`; calls,
//! `invoke`, `landingpad`, `resume`, `va_arg` and the `llvm.va_*`
//! intrinsics are `call`; every other intrinsic is `intrinsic`; `br`,
//! `ret` and `unreachable` are `term`; `extractelement`, `insertelement`
//! and `shufflevector` are `vector` (WP-079); atomic `load` and `store`,
//! `fence`, `cmpxchg` and `atomicrmw` are `atomic` (WP-074). Funclets
//! have no package and are refused here.

use hod_cg::abi::{AbiUnsupported, Assignment, LaneLoc, Part, Sig};
use hod_cg::{
    CallEmit, CgResult, Class, FixupKind, FnCx, FrameDesc, FrameRef, Label, PReg, RegInfo, RegSet,
    Target,
};
use hod_ir::{
    CallSite, Inst, InstKind, Intrinsic, Opcode, ParamAttrs, TypeId, TypeKind, TypeTable, Value,
    ValueKind,
};
use hod_mc_a64 as mc;
use hod_mc_a64::Size;

use crate::common::{
    self, Cursor, V30, V31, X16, X17, copy_bytes, fixup_field, gpr_load_op, load_fpr_at,
    load_gpr_at, other_scratch, put, store_fpr_bytes, store_gpr_bytes, unsupported, vr, xr,
};
use crate::{
    abi, aggr, atomic, call, control, float, frame, int, intrinsic, mem, reg, term, tls, vector,
};

/// The AArch64 Linux target (AAPCS64, `spec/abi-a64.md`). A zero-sized
/// value: everything it knows is in constants.
#[derive(Clone, Copy, Default, Debug)]
pub struct A64;

/// What the target remembers about the function being compiled
/// (`Target::FnState`): the frame's, and one part per group module that
/// needs any. Plain data, reset before each function.
#[derive(Clone, Copy, Default, Debug)]
pub struct FnState {
    /// The frame: prologue, incoming base, result buffer, dynamic
    /// allocation (`frame`, WP-033).
    pub frame: frame::FrameState,
    /// The variadic save area (`call`, WP-038).
    pub va: call::VaState,
    /// The scratch images of vectors (`vector`, WP-079).
    pub vector: vector::VectorState,
    /// Whether instructions are not emitted (`tail`, WP-084).
    pub tail: crate::tail::TailState,
}

/// DWARF numbers: `x<n>` is `n`, `sp` is 31, `v<n>` is `64 + n`.
fn dwarf(reg: PReg) -> u16 {
    if reg::is_fpr(reg) { 64 + u16::from(reg::number(reg)) } else { u16::from(reg.index()) }
}

/// `x<from>`..=`x<to>`.
const fn xs(from: u8, to: u8) -> RegSet {
    let mut set = RegSet::EMPTY;
    let mut n = from;
    while n <= to {
        set = set.with(reg::x(n));
        n += 1;
    }
    set
}

/// `v<from>`..=`v<to>`.
const fn vs(from: u8, to: u8) -> RegSet {
    let mut set = RegSet::EMPTY;
    let mut n = from;
    while n <= to {
        set = set.with(reg::v(n));
        n += 1;
    }
    set
}

/// The registers (§5.5): pool `x9`..`x14`, `v16`..`v29`; holdable also
/// `x0`..`x8`, `x15`, `v0`..`v7`; scratch `x16`, `x17`, `v30`, `v31`;
/// landing-pad data `x0`, `x1`; an empty save set.
pub const REG_INFO: RegInfo = RegInfo {
    gpr: xs(0, 30),
    fpr: vs(0, 31),
    pool: xs(9, 14).union(vs(16, 29)),
    holdable: xs(0, 15).union(vs(16, 29)).union(vs(0, 7)),
    scratch: RegSet::of(&[reg::X16, reg::X17, reg::V30, reg::V31]),
    eh_data: [reg::X0, reg::X1],
    callee: reg::X16,
    gpr_bytes: 8,
    fpr_bytes: 16,
    dwarf,
};

/// The class of a vector type (§2.2): `Fpr` when it fills a `D` or `Q`
/// register as AAPCS64 passes it (a power-of-two count of 8 or 16
/// bytes), `Mem` otherwise.
fn vector_class(types: &TypeTable, ty: TypeId, elem: TypeId, len: u32) -> Class {
    let bits = types.size_in_bits_of(ty);
    let elem_ok = !matches!(types.kind(elem), TypeKind::Int(1));
    if len.is_power_of_two() && (bits == 64 || bits == 128) && elem_ok {
        Class::Fpr
    } else if !elem_ok && len <= crate::mask::MAX_MASK {
        // A mask: bit-packed in a general register (WP-081).
        Class::Gpr
    } else {
        Class::Mem
    }
}

/// The class of the instruction's own result.
fn result_class(cx: &FnCx<'_, A64>, inst: Inst<'_>) -> Class {
    A64.class_of(cx.types(), inst.ty())
}

/// The `call` arm of [`Target::lower`]: a call proper, or the package
/// that owns the intrinsic.
fn lower_call<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, site: &CallSite<'a>) -> CgResult<()> {
    match site.intrinsic() {
        None => call::call(cx, inst),
        Some(which @ (Intrinsic::VaStart | Intrinsic::VaEnd | Intrinsic::VaCopy)) => {
            call::va_intrinsic(cx, inst, which, site.args())
        }
        Some(which @ (Intrinsic::StackSave | Intrinsic::StackRestore)) => {
            mem::stack_intrinsic(cx, inst, which, site.args())
        }
        Some(
            which @ (Intrinsic::SaddWithOverflow
            | Intrinsic::UaddWithOverflow
            | Intrinsic::SsubWithOverflow
            | Intrinsic::UsubWithOverflow
            | Intrinsic::SmulWithOverflow
            | Intrinsic::UmulWithOverflow),
        ) => aggr::overflow_intrinsic(cx, inst, which, site.args()),
        Some(which @ (Intrinsic::ThreadLocalAddress | Intrinsic::ThreadPointer)) => {
            tls::intrinsic(cx, inst, which, site.args())
        }
        Some(which) => intrinsic::intrinsic(cx, inst, which, site.args()),
    }
}

impl Target for A64 {
    type FnState = FnState;

    /// `brk #1`, as `llc` emits for `unreachable` and `llvm.trap`.
    const TRAP: &'static [u8] = &mc::brk(1).to_le_bytes();
    /// `ret`: `__NoopCoro_ResumeDestroy` (WP-105, `crate::coro`).
    const NOOP_CORO_FN: &'static [u8] = &mc::ret(mc::Reg::LR.zr()).to_le_bytes();
    const FN_ALIGN: u32 = 4;
    const PAD_BYTE: u8 = 0;
    const STACK_ALIGN: u32 = 16;
    /// WP-106: `crate::coro_body`.
    const CORO_BODIES: bool = true;

    fn reg_info(&self) -> &RegInfo {
        &REG_INFO
    }

    fn class_of(&self, types: &TypeTable, ty: TypeId) -> Class {
        match types.kind(ty) {
            TypeKind::Int(bits) if bits <= 64 => Class::Gpr,
            TypeKind::Ptr { .. } => Class::Gpr,
            TypeKind::Half
            | TypeKind::BFloat
            | TypeKind::Float
            | TypeKind::Double
            | TypeKind::Fp128 => Class::Fpr,
            TypeKind::Vector { elem, len } => vector_class(types, ty, elem, len),
            _ => Class::Mem,
        }
    }

    fn assign(
        &self,
        types: &TypeTable,
        sig: &Sig<'_>,
        out: &mut Assignment,
    ) -> Result<(), AbiUnsupported> {
        if let Some(assigned) = crate::tail2::assign_preserve_most(types, sig, out) {
            return assigned;
        }
        if let Some(assigned) = crate::tail3::assign_preserve_all(types, sig, out) {
            return assigned;
        }
        abi::assign(types, sig, out)
    }

    fn begin_function(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        crate::tail::begin(cx);
        frame::begin(cx)
    }

    fn end_function(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        frame::end(cx)
    }

    fn frame_base_offset(&self, desc: &FrameDesc, place: FrameRef) -> Option<i64> {
        frame::base_offset(desc, place)
    }

    fn lower<'a>(&self, cx: &mut FnCx<'a, Self>, inst: Inst<'a>) -> CgResult<()> {
        if cx.state.tail.drops && crate::tail::dropped(inst) {
            return Ok(());
        }
        if let Some(done) = crate::tail3::lower(cx, inst) {
            return done;
        }
        if let Some(done) = crate::mask::lower(cx, inst) {
            return done;
        }
        let class = result_class(cx, inst);
        match inst.kind() {
            InstKind::Binary { lhs, rhs } => match class {
                Class::Gpr => int::binary(cx, inst, lhs, rhs),
                Class::Fpr => float::binary(cx, inst, lhs, rhs),
                Class::Mem => aggr::binary(cx, inst, lhs, rhs),
            },
            InstKind::Unary { arg } => match (inst.opcode(), class) {
                (Opcode::FNeg, Class::Fpr) => float::fneg(cx, inst, arg),
                (Opcode::FNeg, Class::Mem) => crate::lanes::fneg(cx, inst, arg),
                (_, Class::Gpr) => int::freeze(cx, inst, arg),
                (_, Class::Fpr) => float::freeze(cx, inst, arg),
                (_, Class::Mem) => aggr::freeze(cx, inst, arg),
            },
            InstKind::Cast { arg } => match (cx.class_of(arg), class) {
                (Class::Mem, _) | (_, Class::Mem) => aggr::cast(cx, inst, arg),
                (Class::Fpr, _) | (_, Class::Fpr) => float::cast(cx, inst, arg),
                (Class::Gpr, Class::Gpr) => int::cast(cx, inst, arg),
            },
            InstKind::ICmp { pred, lhs, rhs } => match cx.class_of(lhs) {
                Class::Gpr => int::icmp(cx, inst, pred, lhs, rhs),
                Class::Fpr => float::vector_icmp(cx, inst, pred, lhs, rhs),
                Class::Mem => aggr::icmp(cx, inst, pred, lhs, rhs),
            },
            InstKind::FCmp { pred, lhs, rhs } => float::fcmp(cx, inst, pred, lhs, rhs),
            InstKind::Load { ptr, mem } if mem.ordering().is_some() => {
                atomic::load(cx, inst, ptr, mem)
            }
            InstKind::Store { value, ptr, mem } if mem.ordering().is_some() => {
                atomic::store(cx, inst, value, ptr, mem)
            }
            InstKind::Load { ptr, mem } => match class {
                Class::Mem => aggr::load(cx, inst, ptr, mem),
                _ => mem::load(cx, inst, ptr, mem),
            },
            InstKind::Store { value, ptr, mem } => match cx.class_of(value) {
                Class::Mem => aggr::store(cx, inst, value, ptr, mem),
                _ => mem::store(cx, inst, value, ptr, mem),
            },
            InstKind::Alloca { count, elem_size, align } => {
                mem::alloca(cx, inst, count, elem_size, align)
            }
            InstKind::PtrAdd(add) => mem::ptr_add(cx, inst, &add),
            InstKind::ExtractValue { agg, offset } => aggr::extract_value(cx, inst, agg, offset),
            InstKind::InsertValue { agg, value, offset } => {
                aggr::insert_value(cx, inst, agg, value, offset)
            }
            InstKind::Select { cond, if_true, if_false } => match class {
                Class::Gpr => int::select(cx, inst, cond, if_true, if_false),
                Class::Fpr => float::select(cx, inst, cond, if_true, if_false),
                Class::Mem => aggr::select(cx, inst, cond, if_true, if_false),
            },
            InstKind::Call(site) => lower_call(cx, inst, &site),
            InstKind::VaArg { list } => call::va_arg(cx, inst, list),
            InstKind::LandingPad(pad) => call::landing_pad(cx, inst, &pad),
            InstKind::Resume { value } => call::resume(cx, inst, value),
            InstKind::Br { target } => term::br(cx, target),
            InstKind::CondBr { cond, if_true, if_false } => {
                control::cond_br(cx, inst, cond, if_true, if_false)
            }
            InstKind::Switch(switch) => control::switch(cx, inst, &switch),
            InstKind::IndirectBr { addr, .. } => control::indirect_br(cx, inst, addr),
            InstKind::Ret { value } => term::ret(cx, inst, value),
            InstKind::Unreachable => term::unreachable(cx),
            InstKind::Fence { ordering, scope } => atomic::fence(cx, ordering, scope),
            InstKind::CmpXchg { ptr, expected, replacement, mem, failure, weak } => {
                atomic::cmpxchg(cx, inst, ptr, expected, replacement, mem, failure, weak)
            }
            InstKind::AtomicRmw { op, ptr, value, mem } => {
                atomic::rmw(cx, inst, op, ptr, value, mem)
            }
            InstKind::ExtractElement { vector, index } => {
                crate::tail2::extract_element(cx, inst, vector, index)
            }
            InstKind::InsertElement { vector, value, index } => {
                vector::insert_element(cx, inst, vector, value, index)
            }
            InstKind::ShuffleVector { first, second, mask } => {
                vector::shuffle_vector(cx, inst, first, second, mask)
            }
            InstKind::Funclet => unsupported(FUNCLET),
            InstKind::Phi(_) => cx.bug("a phi reached the lowering"),
        }
    }

    fn load(&self, cx: &mut FnCx<'_, Self>, reg: PReg, src: FrameRef, bytes: u32) {
        if reg::is_fpr(reg) {
            let mut at = Cursor::frame(cx, src, X16);
            load_fpr_at(cx, common::fpr_view(bytes), vr(reg), &mut at);
        } else {
            let rt = xr(reg);
            let mut at = Cursor::frame(cx, src, other_scratch(rt));
            load_gpr_at(cx, gpr_load_op(bytes), rt, &mut at);
        }
    }

    fn store(&self, cx: &mut FnCx<'_, Self>, reg: PReg, dst: FrameRef, bytes: u32) {
        if reg::is_fpr(reg) {
            let rt = vr(reg);
            let extract = if rt == V31 { V30 } else { V31 };
            let mut at = Cursor::frame(cx, dst, X16);
            store_fpr_bytes(cx, rt, bytes, &mut at, extract);
        } else {
            let rt = xr(reg);
            let mut at = Cursor::frame(cx, dst, other_scratch(rt));
            store_gpr_bytes(cx, rt, bytes, &mut at);
        }
    }

    fn copy(&self, cx: &mut FnCx<'_, Self>, dst: FrameRef, src: FrameRef, bytes: u32) {
        if dst == src || bytes == 0 {
            return;
        }
        let mut from = Cursor::frame(cx, src, X16);
        let mut to = Cursor::frame(cx, dst, X17);
        copy_bytes(cx, &mut to, &mut from, bytes);
    }

    fn copy_from_ptr(
        &self,
        cx: &mut FnCx<'_, Self>,
        dst: FrameRef,
        ptr: PReg,
        offset: u32,
        bytes: u32,
    ) {
        // The policy passes the pointer in `x16`: the source cursor then
        // rebases `x16` itself and moves it back at the end.
        let base = xr(ptr);
        let mut from = Cursor::at_reg(base, i64::from(offset), if base == X17 { X17 } else { X16 });
        let mut to = Cursor::frame(cx, dst, if base == X17 { X16 } else { X17 });
        copy_bytes(cx, &mut to, &mut from, bytes);
        if from.tmp == base {
            from.restore(cx);
        }
    }

    fn mov(&self, cx: &mut FnCx<'_, Self>, dst: PReg, src: PReg) {
        if dst == src {
            return;
        }
        if reg::is_fpr(dst) {
            put(cx, mc::mov_vec(mc::VecBits::V128, vr(dst), vr(src)));
        } else {
            put(cx, mc::mov(Size::X, xr(dst).zr(), xr(src).zr()));
        }
    }

    fn materialize(&self, cx: &mut FnCx<'_, Self>, reg: PReg, value: Value) -> CgResult<()> {
        match value.kind() {
            ValueKind::Const(id) if reg::is_fpr(reg) => {
                common::materialize_fpr_const(cx, vr(reg), id)
            }
            ValueKind::Const(id) => crate::mask::materialize(cx, xr(reg), id),
            ValueKind::Sym(sym) => common::sym_address(cx, xr(reg), sym, 0),
            ValueKind::Slot(slot) => common::slot_address(cx, xr(reg), slot),
            _ => cx.bug("materialize of something that is no constant"),
        }
    }

    fn materialize_mem(
        &self,
        cx: &mut FnCx<'_, Self>,
        dst: FrameRef,
        value: Value,
    ) -> CgResult<()> {
        common::materialize_mem_const(cx, dst, value)
    }

    fn frame_addr(&self, cx: &mut FnCx<'_, Self>, reg: PReg, of: FrameRef) {
        frame::frame_addr(cx, xr(reg), of);
    }

    fn adjust_sp(&self, cx: &mut FnCx<'_, Self>, delta: i64) {
        frame::adjust_sp(cx, delta);
    }

    fn extend_arg(&self, cx: &mut FnCx<'_, Self>, reg: PReg, ty: TypeId, attrs: ParamAttrs) {
        let Some(bits) = cx.types().int_bits(ty) else { return };
        let Some(extension) = abi::produced_extension(bits, attrs) else { return };
        // Wider than 64 bits (`spec/codegen.md` §8.2): `reg` holds the
        // last piece, extended from the integer's bits in it to 64.
        let (sz, width) = if bits > 64 {
            (Size::X, bits % 64)
        } else {
            (common::size_for_bits(extension.to), bits)
        };
        let rd = xr(reg).zr();
        let word = if extension.signed {
            mc::sbfx(sz, rd, rd, 0, width)
        } else {
            mc::ubfx(sz, rd, rd, 0, width)
        };
        put(cx, word);
    }

    fn lanes_out(
        &self,
        cx: &mut FnCx<'_, Self>,
        part: &Part,
        from: LaneLoc,
        to: LaneLoc,
    ) -> CgResult<()> {
        crate::vcall::lanes_out(cx, part, from, to)
    }

    fn lanes_in(
        &self,
        cx: &mut FnCx<'_, Self>,
        part: &Part,
        from: LaneLoc,
        to: LaneLoc,
    ) -> CgResult<()> {
        crate::vcall::lanes_in(cx, part, from, to)
    }

    fn jump(&self, cx: &mut FnCx<'_, Self>, label: Label) -> CgResult<()> {
        common::branch(cx, label)
    }

    fn emit_call(&self, cx: &mut FnCx<'_, Self>, call: &CallEmit) -> CgResult<()> {
        frame::emit_call(cx, call.target);
        Ok(())
    }

    fn dynamic_slot(
        &self,
        cx: &mut FnCx<'_, Self>,
        ptr: FrameRef,
        size: u64,
        align: u64,
    ) -> CgResult<()> {
        frame::dynamic_slot(cx, ptr, size, align)
    }

    fn coro_stubs(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        crate::coro_body::stubs(cx)
    }

    fn coro_dispatch(
        &self,
        cx: &mut FnCx<'_, Self>,
        cond: PReg,
        resume: Label,
        destroy: Label,
    ) -> CgResult<()> {
        crate::coro_body::dispatch(cx, cond, resume, destroy)
    }

    fn patch_branch(code: &mut [u8], at: u32, kind: FixupKind, displacement: i64) -> bool {
        let Some(field) = fixup_field(kind) else { return false };
        let at = at as usize;
        let Some(bytes) = code.get_mut(at..at + 4) else { return false };
        let word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        match field.patch(word, displacement) {
            Some(patched) => {
                bytes.copy_from_slice(&patched.to_le_bytes());
                true
            }
            None => false,
        }
    }
}

/// An atomic `load` or `store` reaching `mem` or `aggr`: never, since
/// WP-074 (`atomic`) takes every atomic instruction.
pub const ATOMIC: &str = "a64:atomic";
/// A vector element operation: lowered by `vector` since WP-079; the
/// name stays for the record.
pub const VECTOR: &str = "a64:vector-element";
/// A funclet pad (Windows exception handling): never on this target.
pub const FUNCLET: &str = "a64:funclet";
/// A thread-local address: milestone M2 (TLS).
pub const THREAD_LOCAL: &str = "a64:thread-local";

#[cfg(test)]
mod tests {
    use super::*;

    /// §5.5: the description satisfies `hod_cg`'s invariants.
    #[test]
    fn reg_info_is_valid() {
        REG_INFO.check().unwrap();
        assert_eq!(REG_INFO.pool.intersection(REG_INFO.gpr).len(), 6);
        assert_eq!(REG_INFO.pool.intersection(REG_INFO.fpr).len(), 14);
        assert!(REG_INFO.holdable.contains(abi::NEST));
        assert!(REG_INFO.holdable.contains(abi::INDIRECT_RESULT));
        assert_eq!(dwarf(reg::V31), 95);
        assert_eq!(dwarf(reg::X30), 30);
        assert_eq!(A64::TRAP, &0xd420_0020u32.to_le_bytes());
    }
}
