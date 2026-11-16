//! The i386 System V calling convention (`spec/codegen-x86.md` "Calls"):
//! WP-293 assigns the `Gpr`-class subset; design 0008's I6 owns this file
//! and extends [`assign`] (attributes, `regparm`/`inreg`, other
//! conventions), I7 the 64-bit integers, I9 the floating-point values,
//! I11 the aggregates.
//!
//! Every argument goes on the stack, in order, each at the next multiple
//! of 4 bytes, its store size wide. A result of `i1`..`i32` or `ptr` is
//! returned in `eax`.

use hod_cg::abi::{AbiUnsupported, Assignment, Part, PartKind, Place, Sig};
use hod_ir::{CallConv, ParamAttrs, ParamMem, TypeId, TypeKind, TypeTable};

use crate::reg::EAX;

/// A calling convention other than C: I6.
pub const CC: &str = "x86:abi:cc";
/// An argument or result attribute this package does not pass: `byval`,
/// `sret`, `inreg`, `nest`, the Swift ones, `inalloca`, `preallocated`,
/// `alignstack`: I6.
pub const ATTR: &str = "x86:abi:attr";

/// The refusal of a value of type `ty` that is no `Gpr` value: by the
/// family that owns it.
pub fn type_refusal(types: &TypeTable, ty: TypeId) -> &'static str {
    match types.kind(ty) {
        TypeKind::Int(_) => crate::int::I64,
        TypeKind::Half
        | TypeKind::BFloat
        | TypeKind::Float
        | TypeKind::Double
        | TypeKind::Fp128
        | TypeKind::X86Fp80 => crate::x87::FLOAT,
        TypeKind::Vector { .. } | TypeKind::ScalableVector { .. } => crate::target::VECTOR,
        _ => crate::target::AGGREGATE,
    }
}

/// Whether `ty` is a `Gpr` value: `i1`..`i32`, `ptr`.
pub fn is_gpr(types: &TypeTable, ty: TypeId) -> bool {
    match types.kind(ty) {
        TypeKind::Int(bits) => bits <= 32,
        TypeKind::Ptr { .. } => true,
        _ => false,
    }
}

/// Whether `attrs` has an attribute this package does not pass.
fn odd(attrs: ParamAttrs) -> bool {
    attrs.has(ParamAttrs::INREG)
        || attrs.has(ParamAttrs::NEST)
        || attrs.has(ParamAttrs::SWIFT_SELF)
        || attrs.has(ParamAttrs::SWIFT_ERROR)
        || attrs.has(ParamAttrs::SWIFT_ASYNC)
        || !matches!(attrs.mem(), ParamMem::None | ParamMem::ElementType)
        || attrs.stack_align().is_some()
}

/// `Target::assign`.
///
/// # Errors
/// [`CC`], [`ATTR`], and the type refusal of a value that is no `Gpr`
/// value.
pub fn assign(
    types: &TypeTable,
    sig: &Sig<'_>,
    out: &mut Assignment,
) -> Result<(), AbiUnsupported> {
    out.clear();
    let refuse = |out: &mut Assignment, name| {
        out.clear();
        Err(AbiUnsupported(name))
    };
    if sig.cc != CallConv::C {
        return refuse(out, CC);
    }
    if sig.ret != TypeId::VOID {
        if !is_gpr(types, sig.ret) {
            return refuse(out, type_refusal(types, sig.ret));
        }
        if odd(sig.ret_attrs) {
            return refuse(out, ATTR);
        }
        let size = types.size_of(sig.ret) as u32;
        out.ret.push(Part {
            arg: 0,
            kind: PartKind::Value,
            offset: 0,
            size,
            place: Place::Reg(EAX),
        });
    }
    let mut at = 0u32;
    for (index, &ty) in sig.params.iter().enumerate() {
        if !is_gpr(types, ty) {
            return refuse(out, type_refusal(types, ty));
        }
        if odd(sig.attrs(index)) {
            return refuse(out, ATTR);
        }
        let size = types.size_of(ty) as u32;
        let arg = index as u32;
        out.args.push(Part {
            arg,
            kind: PartKind::Value,
            offset: 0,
            size,
            place: Place::Stack(at),
        });
        at += size.next_multiple_of(4).max(4);
    }
    out.stack_size = at;
    Ok(())
}
