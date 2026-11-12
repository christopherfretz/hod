//! LLVM's rules for parameter and return attributes (`spec/ir.md` §5.7,
//! §5.8): `verifyParameterAttrs` and `verifyFunctionAttrs`, for a
//! function's signature and for a call's arguments.

use crate::attrs::{ParamAttrs, ParamMem};
use crate::ids::TypeId;
use crate::types::TypeTable;

/// The values whose attributes are checked together: a function's
/// return value and parameters, or a call's result and arguments.
pub(crate) struct Signature<'a> {
    /// The return type and its attributes.
    pub(crate) ret: (TypeId, ParamAttrs),
    /// The number of parameters of the function type; arguments past it
    /// are a call's variable arguments.
    pub(crate) params: usize,
    /// The type of each parameter, or of each argument of a call.
    pub(crate) args: &'a [TypeId],
    /// Whether `elementtype` may appear: on an intrinsic or inline
    /// assembly.
    pub(crate) elementtype: bool,
}

/// Whether `ty` is a pointer or a vector of pointers.
fn pointer_like(types: &TypeTable, ty: TypeId) -> bool {
    types.is_ptr(ty)
        || (types.is_vector(ty) && types.element_type(ty).is_some_and(|elem| types.is_ptr(elem)))
}

fn mem_name(mem: ParamMem) -> &'static str {
    match mem {
        ParamMem::None => "",
        ParamMem::Sret => "sret",
        ParamMem::ByVal => "byval",
        ParamMem::ByRef => "byref",
        ParamMem::InAlloca => "inalloca",
        ParamMem::Preallocated => "preallocated",
        ParamMem::ElementType => "elementtype",
    }
}

/// The attributes `attrs` of one value of type `ty` (`verifyParameterAttrs`).
fn value(
    types: &TypeTable,
    ty: TypeId,
    attrs: ParamAttrs,
    elementtype: bool,
) -> Result<(), String> {
    let mem = attrs.mem();
    if (attrs.has(ParamAttrs::ZEROEXT) || attrs.has(ParamAttrs::SIGNEXT)) && !types.is_int(ty) {
        return Err("zeroext or signext on a value that is not an integer".into());
    }
    if !pointer_like(types, ty) {
        if mem != ParamMem::None {
            return Err(format!("{} on a value that is not a pointer", mem_name(mem)));
        }
        if attrs.has(ParamAttrs::NEST) || attrs.has(ParamAttrs::SWIFT_ERROR) {
            return Err("nest or swifterror on a value that is not a pointer".into());
        }
        if attrs.align().is_some() {
            return Err("align on a value that is not a pointer".into());
        }
    }
    if matches!(
        mem,
        ParamMem::ByVal | ParamMem::ByRef | ParamMem::InAlloca | ParamMem::Preallocated
    ) && !attrs.mem_type().is_some_and(|ty| types.is_sized(ty))
    {
        return Err(format!("{} of a type without a size", mem_name(mem)));
    }
    let count = usize::from(matches!(
        mem,
        ParamMem::ByVal | ParamMem::InAlloca | ParamMem::Preallocated | ParamMem::ByRef
    )) + usize::from(mem == ParamMem::Sret || attrs.has(ParamAttrs::INREG))
        + usize::from(attrs.has(ParamAttrs::NEST));
    if count > 1 {
        return Err(
            "more than one of byval, inalloca, preallocated, byref, sret or inreg, nest".into()
        );
    }
    if mem == ParamMem::ElementType && !elementtype {
        return Err("elementtype is for intrinsics and inline assembly".into());
    }
    Ok(())
}

/// Checks `sig`, with `attrs(i)` the attributes of parameter or argument
/// `i`.
pub(crate) fn signature(
    types: &TypeTable,
    sig: &Signature<'_>,
    attrs: impl Fn(usize) -> ParamAttrs,
) -> Result<(), String> {
    let (ret_ty, ret) = sig.ret;
    if ret.mem() != ParamMem::None {
        return Err(format!("{} on a return value", mem_name(ret.mem())));
    }
    if [ParamAttrs::NEST, ParamAttrs::SWIFT_SELF, ParamAttrs::SWIFT_ERROR, ParamAttrs::SWIFT_ASYNC]
        .into_iter()
        .any(|flag| ret.has(flag))
        || ret.stack_align().is_some()
    {
        return Err("nest, a Swift attribute or alignstack on a return value".into());
    }
    value(types, ret_ty, ret, sig.elementtype)?;
    let (mut sret, mut nest, mut swift) = (false, false, [false; 3]);
    let swift_flags = [ParamAttrs::SWIFT_SELF, ParamAttrs::SWIFT_ASYNC, ParamAttrs::SWIFT_ERROR];
    for (index, &ty) in sig.args.iter().enumerate() {
        let a = attrs(index);
        if a.is_none() {
            continue;
        }
        value(types, ty, a, sig.elementtype)
            .map_err(|what| format!("parameter {index}: {what}"))?;
        let fixed = index < sig.params;
        if a.has(ParamAttrs::NEST) {
            if nest {
                return Err("nest on two parameters".into());
            }
            nest = true;
        }
        if a.mem() == ParamMem::Sret {
            if !fixed {
                return Err("sret on a variable argument".into());
            }
            if sret || index > 1 {
                return Err("sret on a parameter but the first or second, or on two".into());
            }
            sret = true;
        }
        if a.mem() == ParamMem::InAlloca {
            let last = if fixed { sig.params } else { sig.args.len() };
            if index + 1 != last {
                return Err("inalloca on a parameter that is not the last".into());
            }
        }
        if fixed {
            for (seen, flag) in swift.iter_mut().zip(swift_flags) {
                if a.has(flag) {
                    if *seen {
                        return Err("swiftself, swiftasync or swifterror on two parameters".into());
                    }
                    *seen = true;
                }
            }
        }
    }
    Ok(())
}
