//! First-class aggregates (structs and arrays) and the
//! `llvm.*.with.overflow` intrinsics (`spec/codegen-x86.md` §12).
//! **Owner: WP-315** (design 0008 I11, first half). The model is
//! `hod_x64`'s `aggr.rs`, with 32-bit words and §8's two-half integers.
//!
//! # What a slot holds
//!
//! An aggregate is of class `Mem`: its home is a frame slot of its alloc
//! size that holds its in-memory image in the first *store size* bytes.
//! An integer field holds the bits above its width zero when this module
//! wrote it (`insertvalue`, an overflow result, a constant); a field that
//! arrived from another function (an argument, a call result) holds what
//! that function passed. A floating-point field holds its memory width
//! (4, 8, 10 bytes): a value is rounded to it when it is inserted.
//!
//! # Registers
//!
//! A lowering here computes in `eax`, `ecx`, `edx` (no value lives in
//! them) and takes nothing from the pool but its operands. A copy moves
//! pieces of 4, 2 and 1 bytes through `ecx` (`edx` for a pointer in
//! `ecx`). [`materialize_mem`] (glue) writes `ecx` alone and keeps the
//! flags (a zero image above [`REP_ZERO`] bytes saves `eax` and `edi`
//! around a `rep stosb`).

use hod_cg::{CgError, CgResult, Class, FnCx, FrameRef};
use hod_ir::{
    Const, FixupTarget, Inst, InstKind, Intrinsic, MemFlags, TypeId, TypeKind, Value, ValueKind,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Reg, Size};

use crate::alu::{Ext, extend_to};
use crate::common::{
    EAX, ECX, EDX, branch_cond, frame_address, frame_inst, gp, load_gpr, mov_imm, put,
    slot_address, store_at, store_gpr, sym_address, unsupported, value_ty,
};
use crate::target::X86;

/// The widest zero image [`materialize_mem`] writes with immediate
/// stores; a wider one is `rep stosb`.
const REP_ZERO: u32 = 64;

/// Whether `ty` is a struct or an array.
#[must_use]
pub fn is_aggregate(cx: &FnCx<'_, X86>, ty: TypeId) -> bool {
    matches!(cx.types().kind(ty), TypeKind::Struct { .. } | TypeKind::Array { .. })
}

/// Whether `inst` is this module's (besides `extractvalue` and
/// `insertvalue`, which the dispatch sends here): a `load`, `select` or
/// `freeze` of an aggregate, a `store` of one.
#[must_use]
pub fn owns(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    match inst.kind() {
        InstKind::Load { .. } | InstKind::Select { .. } | InstKind::Unary { .. } => {
            is_aggregate(cx, inst.ty())
        }
        InstKind::Store { value, .. } => is_aggregate(cx, value_ty(cx, value)),
        _ => false,
    }
}

/// The lowering of what [`owns`] answers for.
///
/// # Errors
/// What the instruction's lowering reports.
pub fn lower<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>) -> CgResult<()> {
    match inst.kind() {
        InstKind::Load { ptr, mem } => load(cx, inst, ptr, mem),
        InstKind::Store { value, ptr, mem } => store(cx, value, ptr, mem),
        InstKind::Select { cond, if_true, if_false } => select(cx, inst, cond, if_true, if_false),
        InstKind::Unary { arg } => {
            let r = cx.def_mem(inst)?;
            copy_value(cx, r, arg)
        }
        _ => cx.bug("an instruction that is no aggregate's"),
    }
}

// ---- sizes and copies ----

/// The store size of `ty` in bytes, within a frame.
fn store_size(cx: &FnCx<'_, X86>, ty: TypeId) -> CgResult<u32> {
    match u32::try_from(cx.types().size_of(ty)) {
        Ok(bytes) if bytes <= crate::frame::MAX_FRAME => Ok(bytes),
        _ => Err(CgError::unsupported(hod_cg::LIMIT_FRAME_SIZE)),
    }
}

/// The byte offset of a field, within a frame.
fn field_offset(offset: u64) -> CgResult<u32> {
    match u32::try_from(offset) {
        Ok(off) if off <= crate::frame::MAX_FRAME => Ok(off),
        _ => Err(CgError::unsupported(hod_cg::LIMIT_FRAME_SIZE)),
    }
}

/// The pieces of a copy of `bytes` bytes: (offset, size) of 4, 2, 1.
fn pieces(bytes: u32) -> impl Iterator<Item = (u32, u32)> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let size = match bytes - at {
            0 => return None,
            1 => 1,
            2 | 3 => 2,
            _ => 4,
        };
        at += size;
        Some((at - size, size))
    })
}

/// Copies `bytes` from frame place `src` to `[ptr]` through the scratch
/// register that is not `ptr`.
fn copy_to_ptr(cx: &mut FnCx<'_, X86>, ptr: Reg, src: FrameRef, bytes: u32) {
    let t = crate::common::other_scratch(ptr);
    for (at, size) in pieces(bytes) {
        load_gpr(cx, t, src.offset_by(at), size);
        store_at(cx, t, size, t, |k| Mem::base(ptr, at as i32 + k));
    }
}

/// Writes `value` (class `Mem`) into slot `r`: a constant directly, a
/// value by copying its store size.
fn copy_value(cx: &mut FnCx<'_, X86>, r: FrameRef, value: Value) -> CgResult<()> {
    if matches!(value.kind(), ValueKind::Const(_)) {
        return materialize_mem(cx, r, value);
    }
    let bytes = store_size(cx, value_ty(cx, value))?;
    let src = cx.use_mem(value)?;
    if src != r {
        crate::common::copy_frame(cx, r, src, bytes);
    }
    Ok(())
}

/// The address `ptr` in a register: `eax` for a stack slot or a symbol
/// (not thread-local), else the pool register of the value.
fn pointer(cx: &mut FnCx<'_, X86>, ptr: Value) -> CgResult<Reg> {
    match ptr.kind() {
        ValueKind::Slot(slot) => {
            slot_address(cx, EAX, slot)?;
            Ok(EAX)
        }
        ValueKind::Sym(sym)
            if !matches!(
                hod_cg::sym_access(cx.module, cx.options, sym),
                hod_cg::Access::Tls(_)
            ) =>
        {
            sym_address(cx, EAX, sym, 0)?;
            Ok(EAX)
        }
        _ => Ok(gp(cx.use_reg(ptr)?)),
    }
}

/// Refuses a type no field lowering here moves: a vector, `half`,
/// `bfloat`, `fp128`, an integer wider than 64 bits (anywhere inside).
fn check_type(cx: &FnCx<'_, X86>, ty: TypeId) -> CgResult<()> {
    let types = cx.types();
    match types.kind(ty) {
        TypeKind::Int(bits) if bits <= 64 => Ok(()),
        TypeKind::Ptr { .. } | TypeKind::Float | TypeKind::Double | TypeKind::X86Fp80 => Ok(()),
        TypeKind::Array { elem, .. } => check_type(cx, elem),
        TypeKind::Struct { fields, .. } => {
            for &field in fields {
                check_type(cx, field)?;
            }
            Ok(())
        }
        _ => unsupported(crate::abi::type_refusal(types, ty)),
    }
}

// ---- load, store, select ----

/// `%r = load <aggregate>, ptr %ptr`: the store size copied.
///
/// # Errors
/// `x86:atomic`; the refusal of a member type; what the operands report.
fn load<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    if mem.ordering().is_some() {
        return unsupported(crate::mem::ATOMIC);
    }
    check_type(cx, inst.ty())?;
    let bytes = store_size(cx, inst.ty())?;
    let p = pointer(cx, ptr)?;
    let r = cx.def_mem(inst)?;
    crate::common::copy_from_ptr(cx, r, p, 0, bytes);
    Ok(())
}

/// `store <aggregate> %value, ptr %ptr`: the store size copied.
///
/// # Errors
/// As [`load`].
fn store(cx: &mut FnCx<'_, X86>, value: Value, ptr: Value, mem: MemFlags) -> CgResult<()> {
    if mem.ordering().is_some() {
        return unsupported(crate::mem::ATOMIC);
    }
    let ty = value_ty(cx, value);
    check_type(cx, ty)?;
    let bytes = store_size(cx, ty)?;
    // A constant is written to a slot (through `ecx`) before the pointer
    // may be put in `eax`.
    let v = cx.use_mem(value)?;
    let p = pointer(cx, ptr)?;
    copy_to_ptr(cx, p, v, bytes);
    Ok(())
}

/// `%r = select i1 %cond, %a, %b` of aggregates: the address of the
/// chosen operand in `eax` (`test`, a branch), then one copy.
fn select<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    check_type(cx, inst.ty())?;
    if let Some(c) = crate::common::const_int(cx, cond) {
        let r = cx.def_mem(inst)?;
        return copy_value(cx, r, if c & 1 == 1 { if_true } else { if_false });
    }
    let bytes = store_size(cx, inst.ty())?;
    let c = gp(cx.use_reg(cond)?);
    let a = cx.use_mem(if_true)?;
    let b = cx.use_mem(if_false)?;
    let r = cx.def_mem(inst)?;
    let chosen = cx.new_label();
    frame_address(cx, EAX, a);
    put(cx, mc::test_ri(Size::D, c, 1));
    branch_cond(cx, Cond::Ne, chosen)?;
    frame_address(cx, EAX, b);
    cx.bind(chosen)?;
    crate::common::copy_from_ptr(cx, r, EAX, 0, bytes);
    Ok(())
}

// ---- extractvalue, insertvalue ----

/// `%r = extractvalue %agg, <offset>` (the field at byte `offset`): a
/// `Gpr` field loaded into a register, any other copied (its store size).
///
/// # Errors
/// The refusal of the field's type; what the operand reports.
pub fn extract_value<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    agg: Value,
    offset: u64,
) -> CgResult<()> {
    let ty = inst.ty();
    check_type(cx, ty)?;
    let offset = field_offset(offset)?;
    let bytes = store_size(cx, ty)?;
    let a = cx.use_mem(agg)?.offset_by(offset);
    if cx.class_of(inst.result()) == Class::Gpr {
        let rd = cx.def_reg(inst)?;
        load_gpr(cx, gp(rd), a, bytes);
        return cx.finish(inst, rd);
    }
    let r = cx.def_mem(inst)?;
    crate::common::copy_frame(cx, r, a, bytes);
    Ok(())
}

/// `%r = insertvalue %agg, %value, <offset>`: the aggregate copied (a
/// constant one written in place), then the field written with exactly
/// its store size, an integer with the bits above its width zero.
///
/// # Errors
/// As [`extract_value`].
pub fn insert_value<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    agg: Value,
    value: Value,
    offset: u64,
) -> CgResult<()> {
    let ty = value_ty(cx, value);
    check_type(cx, ty)?;
    let offset = field_offset(offset)?;
    let bytes = store_size(cx, ty)?;
    let undef = match value.kind() {
        ValueKind::Const(id) => {
            matches!(cx.module.consts.get(id), Const::Undef(_) | Const::Poison(_))
        }
        _ => false,
    };
    if cx.class_of(value) == Class::Gpr {
        let src = if undef { None } else { Some(gp(cx.use_reg(value)?)) };
        let r = cx.def_mem(inst)?;
        copy_value(cx, r, agg)?;
        if let Some(reg) = src {
            let bits = cx.types().int_bits(ty).unwrap_or(32);
            put(cx, mc::mov_rr(Size::D, EAX, reg));
            extend_to(cx, EAX, bits, Ext::Zero);
            store_gpr(cx, EAX, r.offset_by(offset), bytes);
        }
        return Ok(());
    }
    let src = if undef { None } else { Some(cx.use_mem(value)?) };
    let r = cx.def_mem(inst)?;
    copy_value(cx, r, agg)?;
    let Some(src) = src else { return Ok(()) };
    let to = r.offset_by(offset);
    match crate::int::wide_bits(cx, ty) {
        // The low word, then the high one zero-extended from the width.
        Some(bits) => {
            load_gpr(cx, ECX, src, 4);
            store_gpr(cx, ECX, to, 4);
            load_gpr(cx, ECX, src.offset_by(4), 4);
            extend_to(cx, ECX, bits - 32, Ext::Zero);
            store_gpr(cx, ECX, to.offset_by(4), bytes - 4);
        }
        None => crate::common::copy_frame(cx, to, src, bytes),
    }
    Ok(())
}

// ---- constants ----

/// `Target::materialize_mem` for an aggregate constant: its image
/// (`ConstPool::flatten`) in immediate stores, an address field through
/// `ecx` (`common::sym_address`). Flags kept.
///
/// # Errors
/// `x86:constant` for a constant without an image or with an address
/// field other than a symbol's of 4 bytes; what [`sym_address`] refuses.
pub fn materialize_mem(cx: &mut FnCx<'_, X86>, dst: FrameRef, value: Value) -> CgResult<()> {
    let ValueKind::Const(id) = value.kind() else {
        return cx.bug("materialize_mem of something that is no constant");
    };
    let ty = cx.module.consts.ty(id);
    check_type(cx, ty)?;
    let bytes = store_size(cx, ty)?;
    if cx.module.consts.is_zero_or_undef(id) {
        zero(cx, dst, bytes);
        return Ok(());
    }
    let mut image = Vec::new();
    let mut fixups = Vec::new();
    if cx.module.consts.flatten(cx.types(), id, &mut image, &mut fixups).is_err() {
        return unsupported(crate::common::CONSTANT);
    }
    for (at, size) in pieces(bytes) {
        let start = at as usize;
        let mut word = 0u32;
        for (k, &byte) in image[start..start + size as usize].iter().enumerate() {
            word |= u32::from(byte) << (8 * k);
        }
        let s = match size {
            1 => Size::B,
            2 => Size::W,
            _ => Size::D,
        };
        let imm = if size == 4 { i64::from(word as i32) } else { i64::from(word) };
        frame_inst(cx, dst.offset_by(at), |m| mc::mov_mi(s, m, imm));
    }
    for f in &fixups {
        // An 8-byte address is a `ptrtoint` to `i64`: the 32-bit address
        // zero-extended (as `llc`'s `zext`).
        let sym = match f.target {
            FixupTarget::Sym(sym) | FixupTarget::Plt(sym) if f.size == 4 => sym,
            FixupTarget::Sym(sym) if f.size == 8 => sym,
            _ => return unsupported(crate::common::CONSTANT),
        };
        let at = field_offset(f.offset)?;
        sym_address(cx, ECX, sym, f.addend)?;
        store_gpr(cx, ECX, dst.offset_by(at), 4);
        if f.size == 8 {
            frame_inst(cx, dst.offset_by(at + 4), |m| mc::mov_mi(Size::D, m, 0));
        }
    }
    Ok(())
}

/// Writes zero to `bytes` bytes at `dst`: immediate stores, or for more
/// than [`REP_ZERO`] bytes `rep stosb` with `eax` and `edi` saved (the
/// slot is `ebp`-relative: the pushes do not move it). Flags kept.
fn zero(cx: &mut FnCx<'_, X86>, dst: FrameRef, bytes: u32) {
    if bytes <= REP_ZERO {
        for (at, size) in pieces(bytes) {
            let s = match size {
                1 => Size::B,
                2 => Size::W,
                _ => Size::D,
            };
            frame_inst(cx, dst.offset_by(at), |m| mc::mov_mi(s, m, 0));
        }
        return;
    }
    let edi = Reg::Rdi;
    put(cx, mc::push_r(EAX));
    put(cx, mc::push_r(edi));
    frame_address(cx, edi, dst);
    mov_imm(cx, EAX, 0);
    mov_imm(cx, ECX, bytes);
    put(cx, mc::rep_stos(Size::B));
    put(cx, mc::pop_r(edi));
    put(cx, mc::pop_r(EAX));
}

// ---- a value returned in memory ----

/// `ret` of a function that returns in memory (`spec/abi-x86.md` §5.2):
/// the value's store size copied through the hidden pointer, the first
/// stack argument (loaded into `eax`); the epilogue loads it into `eax`
/// again for the caller (`frame::end`).
///
/// # Errors
/// What the value reports.
pub fn ret_memory(cx: &mut FnCx<'_, X86>, value: Value) -> CgResult<()> {
    let ty = value_ty(cx, value);
    check_type(cx, ty)?;
    let bytes = store_size(cx, ty)?;
    let v = cx.use_mem(value)?;
    load_gpr(cx, EAX, FrameRef::Incoming(0), 4);
    copy_to_ptr(cx, EAX, v, bytes);
    Ok(())
}

// ---- overflow intrinsics ----

/// Whether `which` is one of the six `*.with.overflow` intrinsics.
#[must_use]
pub fn is_overflow(which: Intrinsic<'_>) -> bool {
    matches!(
        which,
        Intrinsic::SaddWithOverflow
            | Intrinsic::UaddWithOverflow
            | Intrinsic::SsubWithOverflow
            | Intrinsic::UsubWithOverflow
            | Intrinsic::SmulWithOverflow
            | Intrinsic::UmulWithOverflow
    )
}

/// The operation of an overflow intrinsic.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
}

/// `%r = call {iN, i1} @llvm.<s|u><add|sub|mul>.with.overflow.iN(a, b)`,
/// `N` from 1 to 64: the result's slot gets the integer (the bits above
/// `N` zero) and the flag as a byte. No call and no branch (`llc` 19 and
/// 22 neither, measured at every width with and without `cmov`).
///
/// # Errors
/// `x86:vector` for vector operands, `x86:i64:wide` above 64 bits; what
/// the operands report.
pub fn overflow<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let &[a, b] = args else { return cx.bug("an overflow intrinsic without two operands") };
    let ty = value_ty(cx, a);
    let bits = match cx.types().kind(ty) {
        TypeKind::Int(bits) if bits <= 64 => bits,
        _ => return unsupported(crate::abi::type_refusal(cx.types(), ty)),
    };
    let Some(&flag_at) = cx.types().field_offsets(inst.ty()).and_then(|o| o.get(1)) else {
        return cx.bug("an overflow result that is no {iN, i1}");
    };
    let flag_at = field_offset(flag_at)?;
    let signed = matches!(
        which,
        Intrinsic::SaddWithOverflow | Intrinsic::SsubWithOverflow | Intrinsic::SmulWithOverflow
    );
    let op = match which {
        Intrinsic::SaddWithOverflow | Intrinsic::UaddWithOverflow => Op::Add,
        Intrinsic::SsubWithOverflow | Intrinsic::UsubWithOverflow => Op::Sub,
        _ => Op::Mul,
    };
    if bits <= 32 {
        overflow_narrow(cx, inst, op, signed, bits, flag_at, a, b)
    } else if op == Op::Mul {
        mul_overflow_wide(cx, inst, signed, bits, flag_at, a, b)
    } else {
        overflow_wide(cx, inst, op, signed, bits, flag_at, a, b)
    }
}

/// The extension of the operands: by sign for the signed operations.
fn ext_of(signed: bool) -> Ext {
    if signed { Ext::Sign } else { Ext::Zero }
}

/// `setcc cl` and the byte to the flag's place.
fn store_flag(cx: &mut FnCx<'_, X86>, cond: Cond, flag: FrameRef) {
    put(cx, mc::setcc_r(cond, ECX));
    frame_inst(cx, flag, |m| mc::mov_mr(Size::B, m, ECX));
}

/// The flag for the exact result in `eax` (`N` < 32, the operands
/// extended): unsigned, any bit at `N` or above set; signed, `eax` is not
/// its own extension from `N` bits. Writes `ecx` and the flag.
fn narrow_flag(cx: &mut FnCx<'_, X86>, signed: bool, bits: u32, flag: FrameRef) {
    put(cx, mc::mov_rr(Size::D, ECX, EAX));
    if signed {
        extend_to(cx, ECX, bits, Ext::Sign);
        put(cx, mc::cmp_rr(Size::D, ECX, EAX));
    } else {
        put(cx, mc::shr_ri(Size::D, ECX, bits as u8));
    }
    store_flag(cx, Cond::Ne, flag);
}

/// Stores the integer result in `eax`, zero-extended from `bits`, with
/// the store size of `iN`.
fn store_narrow_result(cx: &mut FnCx<'_, X86>, r: FrameRef, bits: u32) {
    extend_to(cx, EAX, bits, Ext::Zero);
    store_gpr(cx, EAX, r, bits.div_ceil(8));
}

/// `N` <= 32: the operands extended into `eax` and `ecx`.
#[allow(clippy::too_many_arguments, reason = "the operation's parts, once")]
fn overflow_narrow<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    op: Op,
    signed: bool,
    bits: u32,
    flag_at: u32,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let ext = ext_of(signed);
    let ra = gp(cx.use_reg(a)?);
    let rb = gp(cx.use_reg(b)?);
    let r = cx.def_mem(inst)?;
    let flag = r.offset_by(flag_at);
    put(cx, mc::mov_rr(Size::D, EAX, ra));
    extend_to(cx, EAX, bits, ext);
    put(cx, mc::mov_rr(Size::D, ECX, rb));
    extend_to(cx, ECX, bits, ext);
    match op {
        Op::Add | Op::Sub => {
            put(
                cx,
                if op == Op::Add {
                    mc::add_rr(Size::D, EAX, ECX)
                } else {
                    mc::sub_rr(Size::D, EAX, ECX)
                },
            );
            if bits == 32 {
                store_flag(cx, if signed { Cond::O } else { Cond::B }, flag);
            } else {
                narrow_flag(cx, signed, bits, flag);
            }
        }
        // The product of two values of 16 bits fits 32.
        Op::Mul if bits <= 16 => {
            put(cx, mc::imul_rr(Size::D, EAX, ECX));
            narrow_flag(cx, signed, bits, flag);
        }
        // `edx:eax` is the exact product; at 32 bits `mul`/`imul` set
        // the carry and overflow flags when it is wider.
        Op::Mul if bits == 32 => {
            put(cx, if signed { mc::imul_r(Size::D, ECX) } else { mc::mul_r(Size::D, ECX) });
            store_flag(cx, Cond::O, flag);
        }
        Op::Mul if signed => {
            // The product fits `N` bits when `eax` is its own extension
            // and `edx` the sign of `eax`. `eax` waits in the result's
            // field (its alloc size is 4 from 17 bits on).
            put(cx, mc::imul_r(Size::D, ECX));
            store_gpr(cx, EAX, r, 4);
            put(cx, mc::mov_rr(Size::D, ECX, EAX));
            extend_to(cx, ECX, bits, Ext::Sign);
            put(cx, mc::xor_rr(Size::D, ECX, EAX));
            put(cx, mc::sar_ri(Size::D, EAX, 31));
            put(cx, mc::xor_rr(Size::D, EAX, EDX));
            put(cx, mc::or_rr(Size::D, ECX, EAX));
            store_flag(cx, Cond::Ne, flag);
            load_gpr(cx, EAX, r, 4);
        }
        Op::Mul => {
            put(cx, mc::mul_r(Size::D, ECX));
            put(cx, mc::mov_rr(Size::D, ECX, EAX));
            put(cx, mc::shr_ri(Size::D, ECX, bits as u8));
            put(cx, mc::or_rr(Size::D, ECX, EDX));
            store_flag(cx, Cond::Ne, flag);
        }
    }
    store_narrow_result(cx, r, bits);
    Ok(())
}

/// The high word of the `bits`-bit integer at `slot` into `r`, extended
/// by `ext` from its `bits - 32` bits (flags written).
fn ld_hi(cx: &mut FnCx<'_, X86>, r: Reg, slot: FrameRef, bits: u32, ext: Ext) {
    load_gpr(cx, r, slot.offset_by(4), 4);
    extend_to(cx, r, bits - 32, ext);
}

/// Stores the result `lo` (a register) and `hi` (a register, zero-extended
/// here from `bits - 32` bits; flags written) of `N` > 32 bits.
fn store_wide_result(cx: &mut FnCx<'_, X86>, r: FrameRef, bits: u32, lo: Reg, hi: Reg) {
    store_gpr(cx, lo, r, 4);
    extend_to(cx, hi, bits - 32, Ext::Zero);
    store_gpr(cx, hi, r.offset_by(4), bits.div_ceil(8) - 4);
}

/// `N` from 33 to 64, `add` and `sub`: the high words extended first (no
/// overflow of 64 bits below 64), then `add`/`adc` (`sub`/`sbb`); at 64
/// bits the flags of the high word, else the test of [`narrow_flag`] on
/// the high word.
#[allow(clippy::too_many_arguments, reason = "the operation's parts, once")]
fn overflow_wide<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    op: Op,
    signed: bool,
    bits: u32,
    flag_at: u32,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let ext = ext_of(signed);
    let sa = cx.use_mem(a)?;
    let sb = cx.use_mem(b)?;
    let r = cx.def_mem(inst)?;
    let flag = r.offset_by(flag_at);
    ld_hi(cx, ECX, sa, bits, ext);
    ld_hi(cx, EDX, sb, bits, ext);
    load_gpr(cx, EAX, sa, 4);
    if op == Op::Add {
        frame_inst(cx, sb, |m| mc::add_rm(Size::D, EAX, m));
        put(cx, mc::adc_rr(Size::D, ECX, EDX));
    } else {
        frame_inst(cx, sb, |m| mc::sub_rm(Size::D, EAX, m));
        put(cx, mc::sbb_rr(Size::D, ECX, EDX));
    }
    let cond = if bits == 64 {
        if signed { Cond::O } else { Cond::B }
    } else {
        put(cx, mc::mov_rr(Size::D, EDX, ECX));
        if signed {
            extend_to(cx, EDX, bits - 32, Ext::Sign);
            put(cx, mc::cmp_rr(Size::D, EDX, ECX));
        } else {
            put(cx, mc::shr_ri(Size::D, EDX, (bits - 32) as u8));
        }
        Cond::Ne
    };
    put(cx, mc::setcc_r(cond, EDX));
    frame_inst(cx, flag, |m| mc::mov_mr(Size::B, m, EDX));
    store_wide_result(cx, r, bits, EAX, ECX);
    Ok(())
}

/// `N` from 33 to 64, `mul`: the operands extended to 64 bits (`a'`,
/// `b'`), their 128-bit product `p` by four `mul`s (unsigned), corrected
/// for a signed operation (`a' < 0`: `p -= b' << 64`, and the other way),
/// all in a frame area of its own; overflow when `p` is not its own
/// extension from `N` bits (zero or sign). No libgcc call (`llc` 19 and
/// 22 call none either).
#[allow(clippy::too_many_arguments, reason = "the operation's parts, once")]
fn mul_overflow_wide<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    signed: bool,
    bits: u32,
    flag_at: u32,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let ext = ext_of(signed);
    let sa = cx.use_mem(a)?;
    let sb = cx.use_mem(b)?;
    let area = cx.alloc_slot(32, 4)?;
    let (xa, xb, p) = (area, area.offset_by(8), area.offset_by(16));
    for (from, to) in [(sa, xa), (sb, xb)] {
        load_gpr(cx, ECX, from, 4);
        store_gpr(cx, ECX, to, 4);
        ld_hi(cx, ECX, from, bits, ext);
        store_gpr(cx, ECX, to.offset_by(4), 4);
    }
    for k in 0..4 {
        frame_inst(cx, p.offset_by(4 * k), |m| mc::mov_mi(Size::D, m, 0));
    }
    // (0,0) first, into zero words: no carry is lost past word 3.
    for (i, j) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        let k = i + j;
        load_gpr(cx, EAX, xa.offset_by(4 * i), 4);
        frame_inst(cx, xb.offset_by(4 * j), |m| mc::mul_m(Size::D, m));
        frame_inst(cx, p.offset_by(4 * k), |m| mc::add_mr(Size::D, m, EAX));
        frame_inst(cx, p.offset_by(4 * (k + 1)), |m| mc::adc_mr(Size::D, m, EDX));
        if k + 2 <= 3 {
            frame_inst(cx, p.offset_by(4 * (k + 2)), |m| mc::adc_mi(Size::D, m, 0));
        }
    }
    if signed {
        for (neg, other) in [(xa, xb), (xb, xa)] {
            load_gpr(cx, ECX, neg.offset_by(4), 4);
            put(cx, mc::sar_ri(Size::D, ECX, 31));
            load_gpr(cx, EAX, other, 4);
            put(cx, mc::and_rr(Size::D, EAX, ECX));
            load_gpr(cx, EDX, other.offset_by(4), 4);
            put(cx, mc::and_rr(Size::D, EDX, ECX));
            frame_inst(cx, p.offset_by(8), |m| mc::sub_mr(Size::D, m, EAX));
            frame_inst(cx, p.offset_by(12), |m| mc::sbb_mr(Size::D, m, EDX));
        }
        // `t` = word 1 extended from `N - 32` bits; overflow when `t`
        // differs from word 1 or a word above is not `t`'s sign.
        load_gpr(cx, EAX, p.offset_by(4), 4);
        extend_to(cx, EAX, bits - 32, Ext::Sign);
        put(cx, mc::mov_rr(Size::D, ECX, EAX));
        frame_inst(cx, p.offset_by(4), |m| mc::xor_rm(Size::D, ECX, m));
        put(cx, mc::sar_ri(Size::D, EAX, 31));
        for k in [2, 3] {
            load_gpr(cx, EDX, p.offset_by(4 * k), 4);
            put(cx, mc::xor_rr(Size::D, EDX, EAX));
            put(cx, mc::or_rr(Size::D, ECX, EDX));
        }
    } else {
        // Overflow when a bit at `N` or above is set.
        if bits == 64 {
            mov_imm(cx, ECX, 0);
        } else {
            load_gpr(cx, ECX, p.offset_by(4), 4);
            put(cx, mc::shr_ri(Size::D, ECX, (bits - 32) as u8));
        }
        frame_inst(cx, p.offset_by(8), |m| mc::or_rm(Size::D, ECX, m));
        frame_inst(cx, p.offset_by(12), |m| mc::or_rm(Size::D, ECX, m));
    }
    let r = cx.def_mem(inst)?;
    store_flag(cx, Cond::Ne, r.offset_by(flag_at));
    load_gpr(cx, EAX, p, 4);
    load_gpr(cx, EDX, p.offset_by(4), 4);
    store_wide_result(cx, r, bits, EAX, EDX);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_cover_the_bytes() {
        let all: Vec<_> = pieces(7).collect();
        assert_eq!(all, [(0, 4), (4, 2), (6, 1)]);
        assert_eq!(pieces(0).count(), 0);
        assert_eq!(pieces(8).map(|(_, s)| s).sum::<u32>(), 8);
    }
}
