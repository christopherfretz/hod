//! The bodies of variadic functions (`spec/codegen-x86.md` §12,
//! `spec/abi-x86.md` §9; design 0008's I11, WP-316): `llvm.va_start`,
//! `llvm.va_copy`, `llvm.va_end` and the `va_arg` instruction.
//!
//! On i386 a `va_list` is one pointer into the caller's stack arguments
//! (`spec/abi-x86.md` §9.2). `va_start` stores the address of the first
//! variable argument, `Incoming(VaInfo::stack_offset)`; `va_copy` copies
//! the pointer; `va_end` emits nothing. clang 19 and 22 expand `va_arg`
//! to loads and pointer arithmetic themselves; the IR instruction (which
//! hand-written IR and other front ends use) is lowered as `llc` 19 and
//! 22 expand it (`TargetLowering::expandVAArg`, measured): the pointer
//! is rounded up to the type's ABI alignment, the value read there, and
//! the pointer advanced by the type's allocation size (`i8` by 1, `i64`
//! and `double` by 8 at alignment 4, `x86_fp80` by 12 at alignment 4).

use hod_cg::{CgResult, FnCx, FrameRef};
use hod_ir::{Inst, Intrinsic, TypeKind, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Mem, Size};

use crate::common::{EAX, ECX, EDX, frame_address, gp, load_at, put, store_gpr, unsupported};
use crate::target::X86;

/// `va_start` in a function that is not variadic, and an intrinsic call
/// of the family with an operand list it does not have (invalid IR).
pub const VARARGS: &str = crate::call::VARARGS;
/// A `va_arg` of a type the i386 convention does not pass as one value
/// that this module reads: an aggregate, a vector, `half`, `bfloat`,
/// `fp128`, an integer wider than 64 bits (clang never writes one: it
/// expands `va_arg` itself).
pub const VA_ARG_TYPE: &str = "x86:varargs:va-arg-type";

/// Whether the function being compiled is variadic.
fn is_variadic(cx: &FnCx<'_, X86>) -> bool {
    matches!(cx.types().kind(cx.body.fn_type()), TypeKind::Function { variadic: true, .. })
}

/// `llvm.va_start`, `llvm.va_copy`, `llvm.va_end` (`args` the call's
/// operands).
///
/// # Errors
/// [`VARARGS`] for `va_start` outside a variadic function; what the
/// operands report.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X86>,
    _inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    match (which, args) {
        (Intrinsic::VaEnd, [_]) => Ok(()),
        (Intrinsic::VaStart, &[list]) if is_variadic(cx) => {
            let first = cx.fn_assignment().va.stack_offset;
            let l = gp(cx.use_reg(list)?);
            frame_address(cx, ECX, FrameRef::Incoming(first));
            put(cx, mc::mov_mr(Size::D, Mem::base(l, 0), ECX));
            Ok(())
        }
        (Intrinsic::VaCopy, &[dst, src]) => {
            let s = gp(cx.use_reg(src)?);
            put(cx, mc::mov_rm(Size::D, ECX, Mem::base(s, 0)));
            let d = gp(cx.use_reg(dst)?);
            put(cx, mc::mov_mr(Size::D, Mem::base(d, 0), ECX));
            Ok(())
        }
        _ => unsupported(VARARGS),
    }
}

/// How `va_arg` reads a value of a type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layout {
    /// The bytes the pointer advances by (the allocation size).
    pub step: u32,
    /// The alignment the pointer is rounded up to first.
    pub align: u32,
    /// The bytes read (the store size).
    pub bytes: u32,
    /// Whether the value is an integer of 33 to 64 bits (two halves).
    pub wide: bool,
}

/// The [`Layout`] of `ty` under the i386 data layout, `None` for a type
/// [`VA_ARG_TYPE`] refuses.
pub fn layout(cx: &FnCx<'_, X86>, ty: hod_ir::TypeId) -> Option<Layout> {
    let types = cx.types();
    let plain = |step: u32, align: u32, bytes: u32| Layout { step, align, bytes, wide: false };
    match types.kind(ty) {
        TypeKind::Ptr { .. } => Some(plain(4, 4, 4)),
        TypeKind::Float => Some(plain(4, 4, 4)),
        TypeKind::Double => Some(plain(8, 4, 8)),
        TypeKind::X86Fp80 => Some(plain(12, 4, 10)),
        _ => match types.int_bits(ty)? {
            0 => None,
            1..=8 => Some(plain(1, 1, 1)),
            9..=16 => Some(plain(2, 2, 2)),
            17..=24 => Some(plain(4, 4, 3)),
            25..=32 => Some(plain(4, 4, 4)),
            bits @ 33..=64 => {
                Some(Layout { step: 8, align: 4, bytes: bits.div_ceil(8), wide: true })
            }
            _ => None,
        },
    }
}

/// `%r = va_arg ptr %list, <ty>`: `ecx` = the rounded pointer, the
/// advanced pointer stored back through `list` (by `edx`), then the
/// value read at `[ecx]`.
///
/// # Errors
/// [`VA_ARG_TYPE`]; what the operands report.
pub fn va_arg<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, list: Value) -> CgResult<()> {
    let Some(at) = layout(cx, inst.ty()) else { return unsupported(VA_ARG_TYPE) };
    let l = gp(cx.use_reg(list)?);
    put(cx, mc::mov_rm(Size::D, ECX, Mem::base(l, 0)));
    if at.align > 1 {
        put(cx, mc::add_ri(Size::D, ECX, i64::from(at.align - 1)));
        put(cx, mc::and_ri(Size::D, ECX, -i64::from(at.align)));
    }
    put(cx, mc::lea(Size::D, EDX, Mem::base(ECX, at.step as i32)));
    put(cx, mc::mov_mr(Size::D, Mem::base(l, 0), EDX));
    if crate::abi::is_gpr(cx.types(), inst.ty()) {
        let rd = cx.def_reg(inst)?;
        load_at(cx, gp(rd), at.bytes, EDX, |k| Mem::base(ECX, k));
        return cx.finish(inst, rd);
    }
    let dst = cx.def_mem(inst)?;
    if at.wide {
        // `spec/codegen-x86.md` §8: two whole words, the high one
        // zero-extended from the store size.
        load_at(cx, EAX, 4, EDX, |k| Mem::base(ECX, k));
        store_gpr(cx, EAX, dst, 4);
        load_at(cx, EAX, at.bytes - 4, EDX, |k| Mem::base(ECX, 4 + k));
        store_gpr(cx, EAX, dst.offset_by(4), 4);
    } else {
        crate::common::copy_from_ptr(cx, dst, ECX, 0, at.bytes);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_are_stable() {
        assert_eq!(super::VARARGS, "x86:varargs");
        assert_eq!(super::VA_ARG_TYPE, "x86:varargs:va-arg-type");
    }
}
