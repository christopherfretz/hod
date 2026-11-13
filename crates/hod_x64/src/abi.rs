//! `assign`: where arguments and return values travel (System V AMD64
//! psABI). Owned by WP-118, which writes `spec/abi-x64.md` and replaces
//! this file whole.
//!
//! What is here is WP-116's stub, enough for the skeleton's tests: `ccc`,
//! not variadic, up to six arguments of class `Gpr` (integers of at most
//! 64 bits, pointers) in `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`, and a
//! result of that class in `rax`. Everything else is refused as
//! [`ABI`]; a `nest` argument as [`NEST`] (`r10` is a scratch register,
//! design 0006 §2.1).

use hod_cg::abi::{AbiUnsupported, Assignment, Part, PartKind, Place, Sig};
use hod_ir::{CallConv, ParamAttrs, ParamMem, TypeId, TypeKind, TypeTable};

use crate::reg::{R8, R9, RAX, RCX, RDI, RDX, RSI};

/// A signature the stub does not assign: WP-118.
pub const ABI: &str = "x64:todo:abi";
/// A `nest` argument: `r10`, which is a scratch register here.
pub const NEST: &str = "x64:nest";

/// The integer argument registers, in order.
pub const INT_ARGS: [hod_cg::PReg; 6] = [RDI, RSI, RDX, RCX, R8, R9];

/// Where the arguments and the result of `sig` travel; `out` is cleared
/// first, and left empty on an error.
///
/// # Errors
/// [`ABI`] for what the stub does not assign, [`NEST`] for `nest`.
pub fn assign(
    types: &TypeTable,
    sig: &Sig<'_>,
    out: &mut Assignment,
) -> Result<(), AbiUnsupported> {
    out.clear();
    let result = assign_signature(types, sig, out);
    if result.is_err() {
        out.clear();
    }
    result
}

fn assign_signature(
    types: &TypeTable,
    sig: &Sig<'_>,
    out: &mut Assignment,
) -> Result<(), AbiUnsupported> {
    if sig.cc != CallConv::C || sig.variadic || sig.params.len() > INT_ARGS.len() {
        return Err(AbiUnsupported(ABI));
    }
    if sig.ret != TypeId::VOID {
        let size = scalar_size(types, sig.ret, sig.ret_attrs)?;
        out.ret.push(part(0, size, RAX));
    }
    for (index, (&ty, reg)) in sig.params.iter().zip(INT_ARGS).enumerate() {
        let size = scalar_size(types, ty, sig.attrs(index))?;
        out.args.push(part(index as u32, size, reg));
    }
    Ok(())
}

/// The size in bytes of an argument or result the stub passes: an
/// integer of at most 64 bits or a pointer, with no attribute that
/// changes how it travels.
fn scalar_size(types: &TypeTable, ty: TypeId, attrs: ParamAttrs) -> Result<u32, AbiUnsupported> {
    if attrs.has(ParamAttrs::NEST) {
        return Err(AbiUnsupported(NEST));
    }
    let plain = attrs.mem() == ParamMem::None
        && !attrs.has(ParamAttrs::INREG)
        && !attrs.has(ParamAttrs::SWIFT_SELF)
        && !attrs.has(ParamAttrs::SWIFT_ERROR)
        && !attrs.has(ParamAttrs::SWIFT_ASYNC);
    let scalar = match types.kind(ty) {
        TypeKind::Int(bits) => (1..=64).contains(&bits),
        TypeKind::Ptr { .. } => true,
        _ => false,
    };
    if !(plain && scalar) {
        return Err(AbiUnsupported(ABI));
    }
    Ok(types.alloc_size_of(ty).clamp(1, 8) as u32)
}

fn part(arg: u32, size: u32, reg: hod_cg::PReg) -> Part {
    Part { arg, kind: PartKind::Value, offset: 0, size, place: Place::Reg(reg) }
}
