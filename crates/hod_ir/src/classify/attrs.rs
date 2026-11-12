//! The parameter attributes a call passes with (`spec/codegen.md` §7.1,
//! "Effective attributes"): the call site's, plus those of the function
//! a direct call names when the call's function type is that function's
//! type. `classify` and the call sequence of `hod_cg` both read a call's
//! attributes through these functions, so that they cannot disagree.
//!
//! The rule is LLVM's, as `llc` lowers a call (`CallBase::paramHasAttr`
//! and `getCalledFunction`, measured with `llc` 19 and 22 on AArch64):
//!
//! - A call through a function type other than the callee's takes
//!   nothing from the declaration.
//! - Otherwise each flag (`zeroext`, `signext`, `inreg`, `nest`,
//!   `swiftself`, `swifterror`, `swiftasync`) is set if either side has
//!   it; `signext` wins over `zeroext`.
//! - The typed attribute (`sret`, `byval`, ...) is the call site's if it
//!   has one, else the declaration's, with its type. Two different kinds
//!   that both change how the argument is passed (`sret`, `byval`,
//!   `inalloca`, `preallocated`) are an [`AttrConflict`]; `byref` and
//!   `elementtype` change nothing about passing and yield to the other.
//! - `align` and `alignstack` are the call site's alone: `llc` ignores a
//!   declaration's even when the types match.

use crate::attrs::{ParamAttrs, ParamMem};
use crate::body::CallSite;
use crate::module::Module;
use crate::symbol::{Symbol, SymbolKind};
use crate::value::ValueKind;

/// A call site and a declaration that give one argument two different
/// typed attributes that both change how it is passed (`sret` on one
/// side, `byval` on the other). `llc` passes such an argument as `sret`
/// on AArch64 whichever side has it; hod does not encode that
/// precedence and refuses the call under [`AttrConflict::NAME`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttrConflict;

impl AttrConflict {
    /// The stable name of the refusal.
    pub const NAME: &'static str = "call:attribute-conflict";
}

/// The function whose declared attributes a call takes besides its own:
/// the function a direct call names, if the call's function type is that
/// function's type. `None` for an indirect call, a call of an alias or a
/// variable, and a call through another function type.
pub fn call_declaration<'m>(module: &'m Module, site: &CallSite<'_>) -> Option<Symbol<'m>> {
    let ValueKind::Sym(sym) = site.callee_value().kind() else { return None };
    if sym.index() >= module.symbol_count() {
        return None;
    }
    let symbol = module.symbol(sym);
    let function = matches!(symbol.kind(), SymbolKind::Function { .. });
    (function && symbol.ty() == site.fn_type()).then_some(symbol)
}

/// The attributes the call passes argument `index` with.
///
/// # Errors
/// [`AttrConflict`].
pub fn call_arg_attrs(
    module: &Module,
    site: &CallSite<'_>,
    index: usize,
) -> Result<ParamAttrs, AttrConflict> {
    let own = site.arg_attrs(index);
    match call_declaration(module, site) {
        Some(declared) => merge_call_attrs(own, declared.param_attrs(index)),
        None => Ok(own),
    }
}

/// The attributes the call takes its result with.
///
/// # Errors
/// [`AttrConflict`].
pub fn call_ret_attrs(module: &Module, site: &CallSite<'_>) -> Result<ParamAttrs, AttrConflict> {
    let own = site.ret_attrs();
    match call_declaration(module, site).map(|declared| declared.kind()) {
        Some(SymbolKind::Function { ret_attrs, .. }) => merge_call_attrs(own, ret_attrs),
        _ => Ok(own),
    }
}

/// The flags a declaration contributes to a call's attributes.
const FLAGS: [u16; 7] = [
    ParamAttrs::ZEROEXT,
    ParamAttrs::SIGNEXT,
    ParamAttrs::INREG,
    ParamAttrs::NEST,
    ParamAttrs::SWIFT_SELF,
    ParamAttrs::SWIFT_ERROR,
    ParamAttrs::SWIFT_ASYNC,
];

/// Whether a typed attribute changes how its argument is passed.
const fn passes(mem: ParamMem) -> bool {
    matches!(mem, ParamMem::Sret | ParamMem::ByVal | ParamMem::InAlloca | ParamMem::Preallocated)
}

/// The attributes of one argument (or of the result) of a call whose
/// site has `site` and whose declaration, consulted by
/// [`call_declaration`], has `declared`: the rule of the module
/// documentation.
///
/// # Errors
/// [`AttrConflict`].
pub fn merge_call_attrs(
    site: ParamAttrs,
    declared: ParamAttrs,
) -> Result<ParamAttrs, AttrConflict> {
    if declared.is_none() || declared == site {
        return Ok(site);
    }
    let mut flags = 0;
    for flag in FLAGS {
        if site.has(flag) || declared.has(flag) {
            flags |= flag;
        }
    }
    if flags & ParamAttrs::SIGNEXT != 0 {
        flags &= !ParamAttrs::ZEROEXT;
    }
    let (mine, theirs) = (site.mem(), declared.mem());
    let typed = if passes(mine) {
        if passes(theirs) && theirs != mine {
            return Err(AttrConflict);
        }
        site
    } else if passes(theirs) || mine == ParamMem::None {
        declared
    } else {
        site
    };
    let fail = |_| AttrConflict;
    let mut out = ParamAttrs::NONE.with(flags).map_err(fail)?;
    if let Some(ty) = typed.mem_type() {
        out = out.with_mem(typed.mem(), ty).map_err(fail)?;
    }
    if let Some(bytes) = site.align() {
        out = out.with_align(bytes).map_err(fail)?;
    }
    if let Some(bytes) = site.stack_align() {
        out = out.with_stack_align(bytes).map_err(fail)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::TypeId;

    fn flags(bits: u16) -> ParamAttrs {
        ParamAttrs::NONE.with(bits).unwrap()
    }

    /// The rows of the module documentation, one by one.
    #[test]
    fn the_merge_rule() {
        let sext = flags(ParamAttrs::SIGNEXT);
        let zext = flags(ParamAttrs::ZEROEXT);
        assert_eq!(merge_call_attrs(ParamAttrs::NONE, sext), Ok(sext));
        assert_eq!(merge_call_attrs(zext, sext), Ok(sext));
        assert_eq!(merge_call_attrs(sext, zext), Ok(sext));
        let nest = flags(ParamAttrs::NEST);
        assert_eq!(merge_call_attrs(nest, sext), Ok(flags(ParamAttrs::NEST | ParamAttrs::SIGNEXT)));

        let byval = |ty| ParamAttrs::NONE.with_mem(ParamMem::ByVal, ty).unwrap();
        let sret = ParamAttrs::NONE.with_mem(ParamMem::Sret, TypeId::I64).unwrap();
        let byref = ParamAttrs::NONE.with_mem(ParamMem::ByRef, TypeId::I32).unwrap();
        let aligned = ParamAttrs::NONE.with_align(8).unwrap();
        // The declaration's `byval` with its type, the site's `align`.
        assert_eq!(
            merge_call_attrs(aligned, byval(TypeId::I64)),
            Ok(byval(TypeId::I64).with_align(8).unwrap())
        );
        // The site's type wins.
        assert_eq!(
            merge_call_attrs(byval(TypeId::I32), byval(TypeId::I64)),
            Ok(byval(TypeId::I32))
        );
        assert_eq!(merge_call_attrs(byref, byval(TypeId::I64)), Ok(byval(TypeId::I64)));
        assert_eq!(merge_call_attrs(byval(TypeId::I32), byref), Ok(byval(TypeId::I32)));
        assert_eq!(merge_call_attrs(sret, byval(TypeId::I64)), Err(AttrConflict));
        assert_eq!(merge_call_attrs(byval(TypeId::I64), sret), Err(AttrConflict));
        // A declaration's `align` and `alignstack` are not taken.
        let stack = ParamAttrs::NONE.with_stack_align(16).unwrap().with_align(16).unwrap();
        assert_eq!(merge_call_attrs(ParamAttrs::NONE, stack), Ok(ParamAttrs::NONE));
        assert_eq!(
            merge_call_attrs(ParamAttrs::NONE, byval(TypeId::I64).with_align(16).unwrap()),
            Ok(byval(TypeId::I64))
        );
    }
}
