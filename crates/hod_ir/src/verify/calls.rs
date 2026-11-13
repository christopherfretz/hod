//! The rules of LLVM's verifier for calls that `Body::verify` keeps
//! (`spec/ir.md` §5.8): what each opcode may call, tail markers and
//! `musttail`'s conditions, immediate arguments, operand bundles.

use super::{Signature, Verifier, signature};
use crate::asm::InlineAsm;
use crate::attrs::{CallConv, ParamAttrs, ParamMem};
use crate::body::{BundleData, CALL_ALWAYS_INLINE, CALL_NOINLINE, CALL_TAIL_MASK, CallData, UNSET};
use crate::constant::Const;
use crate::error::IrErrorKind::{InvalidAttribute, InvalidCall, InvalidOperand};
use crate::error::IrResult;
use crate::inst::{Opcode, TailKind};
use crate::intern::Layering;
use crate::intrinsic::{CODE_OTHER, Intrinsic};
use crate::symbol::SymbolKind;
use crate::types::TypeKind;
use crate::value::{Value, ValueKind};

/// The bundle tags LLVM allows once per call.
const SINGLE_BUNDLES: &[&str] = &[
    "deopt",
    "funclet",
    "gc-transition",
    "cfguardtarget",
    "preallocated",
    "gc-live",
    "ptrauth",
    "kcfi",
    "clang.arc.attachedcall",
    "convergencectrl",
];

/// The number of label constraints (`!i`) in an inline-assembly
/// constraint string.
fn labels(constraints: &[u8]) -> usize {
    constraints.split(|&byte| byte == b',').filter(|part| *part == b"!i").count()
}

/// The parameter attributes LLVM compares between a `musttail` call and
/// its caller (`getParameterABIAttributes`): the typed ones but
/// `elementtype`, `inreg`, `alignstack`, the Swift ones, and `align` with
/// `byval` or `byref`.
fn abi(attrs: ParamAttrs) -> (ParamMem, Option<crate::ids::TypeId>, u16, Option<u64>, Option<u64>) {
    let mem = attrs.mem();
    let (mem, ty) =
        if mem == ParamMem::ElementType { (ParamMem::None, None) } else { (mem, attrs.mem_type()) };
    let flags = [
        ParamAttrs::INREG,
        ParamAttrs::SWIFT_SELF,
        ParamAttrs::SWIFT_ASYNC,
        ParamAttrs::SWIFT_ERROR,
    ]
    .into_iter()
    .filter(|&flag| attrs.has(flag))
    .fold(0, |all, flag| all | flag);
    let align = matches!(mem, ParamMem::ByVal | ParamMem::ByRef).then(|| attrs.align()).flatten();
    (mem, ty, flags, attrs.stack_align(), align)
}

/// Whether `attrs` has an attribute LLVM refuses in a `tailcc` or
/// `swifttailcc` `musttail` call or caller.
fn tailcc_refuses(attrs: ParamAttrs) -> bool {
    matches!(attrs.mem(), ParamMem::InAlloca | ParamMem::Preallocated | ParamMem::ByRef)
        || attrs.has(ParamAttrs::INREG)
        || attrs.has(ParamAttrs::SWIFT_ERROR)
}

impl<L: Layering> Verifier<'_, L> {
    /// The intrinsic a call record names, if any.
    pub(super) fn intrinsic_of(&self, call: &CallData) -> Option<Intrinsic<'_>> {
        let body = self.body;
        match call.intrinsic {
            0 => None,
            CODE_OTHER => {
                let start = body.words[call.extra as usize] as usize;
                let len = body.words[call.extra as usize + 1] as usize;
                Some(Intrinsic::Other(&body.names[start..start + len]))
            }
            code => Intrinsic::from_code(code),
        }
    }

    /// The calling convention and parameter attributes of the function
    /// whose body this is: its symbol's, when the module has the symbol
    /// as a function; otherwise the default convention and none.
    fn caller(&self) -> (CallConv, &[ParamAttrs]) {
        let sym = self.body.symbol();
        if sym.index() < self.module.symbol_count()
            && let SymbolKind::Function { cc, param_attrs, .. } = self.module.symbol(sym).kind()
        {
            return (cc, param_attrs);
        }
        (CallConv::C, &[])
    }

    /// The attributes of argument `index` of a call (or `None`, the
    /// return value), `NONE` where it has none.
    pub(super) fn call_attrs(&self, call: &CallData, index: Option<usize>) -> ParamAttrs {
        if call.attrs == UNSET {
            return ParamAttrs::NONE;
        }
        let at = call.attrs as usize + index.map_or(0, |index| index + 1);
        self.body.param_attrs[at]
    }

    /// LLVM's rules for the call `id` (§5.8), after its shape and types
    /// are checked.
    pub(super) fn call_rules(
        &self,
        id: u32,
        op: Opcode,
        call: &CallData,
        vals: &[Value],
        bundles: &[BundleData],
        block: crate::ids::BlockId,
    ) -> IrResult<()> {
        let body = self.body;
        let (callee, args) = (vals[0], &vals[1..=call.nargs as usize]);
        let tail = TailKind::from_raw(call.flags & CALL_TAIL_MASK);
        let both = CALL_ALWAYS_INLINE | CALL_NOINLINE;
        if call.flags & both == both {
            return self.fail(InvalidAttribute, id, "'noinline and alwaysinline' are incompatible");
        }
        let intrinsic = self.intrinsic_of(call);
        let asm = match callee.kind() {
            ValueKind::Asm(asm) if asm.index() < self.module.asm_count() => {
                Some(self.module.asm(asm))
            }
            ValueKind::Asm(asm) => {
                return self.fail(
                    InvalidOperand,
                    id,
                    format!("{asm:?} is not inline assembly of the module"),
                );
            }
            _ => None,
        };
        if call.attrs != UNSET {
            let types: Vec<_> = args.iter().map(|&arg| body.value_type(self.module, arg)).collect();
            let TypeKind::Function { ret, params, .. } = self.types.kind(call.fn_ty) else {
                return self.fail(InvalidOperand, id, "the call's type is not a function type");
            };
            let sig = Signature {
                ret: (ret, self.call_attrs(call, None)),
                params: params.len(),
                args: &types,
                elementtype: intrinsic.is_some() || asm.is_some(),
            };
            if let Err(what) =
                signature(self.types, &sig, |index| self.call_attrs(call, Some(index)))
            {
                return self.fail(InvalidAttribute, id, what);
            }
        }
        let labels = asm.map_or(0, |asm| labels(asm.constraints));
        if op == Opcode::CallBr {
            let Some(asm) = asm else {
                return self.fail(InvalidCall, id, "callbr calls inline assembly only");
            };
            if asm.flags & InlineAsm::UNWIND != 0 {
                return self.fail(InvalidCall, id, "callbr of inline assembly that unwinds");
            }
            let dests = body.words[call.unwind as usize] as usize - 1;
            if labels != dests {
                let what = format!("{labels} label constraints for {dests} indirect destinations");
                return self.fail(InvalidCall, id, what);
            }
        } else if labels != 0 {
            return self.fail(InvalidCall, id, "label constraints are for callbr only");
        }
        if op == Opcode::Invoke && intrinsic.is_some_and(|intrinsic| !intrinsic.is_invokable()) {
            let name = intrinsic.map_or("", Intrinsic::name);
            return self.fail(InvalidCall, id, format!("{name} cannot be invoked"));
        }
        if tail != TailKind::None && op != Opcode::Call {
            return self.fail(InvalidCall, id, format!("{} on {}", tail.name(), op.name()));
        }
        if tail == TailKind::MustTail {
            self.must_tail(id, call, asm.is_some(), intrinsic.is_some(), block)?;
        }
        if let Some(intrinsic) = intrinsic {
            for &index in intrinsic.immediate_args() {
                let immediate = args[index].as_const().is_some_and(|constant| {
                    matches!(
                        self.module.consts.get(constant),
                        Const::Int { .. }
                            | Const::WideInt { .. }
                            | Const::Float { .. }
                            | Const::WideFloat { .. }
                    )
                });
                if !immediate {
                    let what =
                        format!("argument {index} of {} is not a constant", intrinsic.name());
                    return self.fail(InvalidCall, id, what);
                }
            }
        }
        for (index, bundle) in bundles.iter().enumerate() {
            let tag = self.bundle_tag(bundle);
            if SINGLE_BUNDLES.contains(&tag)
                && bundles[..index].iter().any(|earlier| self.bundle_tag(earlier) == tag)
            {
                return self.fail(InvalidCall, id, format!("two {tag} bundles"));
            }
            if tag == "kcfi" {
                let inputs = body.val_list(bundle.first, bundle.count);
                let ok = matches!(inputs, [input] if input.as_const().is_some_and(|constant| {
                    matches!(self.module.consts.get(constant), Const::Int { ty, .. } if ty == crate::ids::TypeId::I32)
                }));
                if !ok {
                    return self.fail(InvalidCall, id, "a kcfi bundle has one i32 constant");
                }
            }
        }
        Ok(())
    }

    fn bundle_tag(&self, bundle: &BundleData) -> &str {
        &self.body.names[bundle.tag_start as usize..][..bundle.tag_len as usize]
    }

    /// `musttail`: LLVM's `verifyMustTailCall`.
    fn must_tail(
        &self,
        id: u32,
        call: &CallData,
        asm: bool,
        intrinsic: bool,
        block: crate::ids::BlockId,
    ) -> IrResult<()> {
        let body = self.body;
        let fail = |what: &str| self.fail(InvalidCall, id, format!("musttail: {what}"));
        if asm {
            return fail("the callee is inline assembly");
        }
        let TypeKind::Function {
            ret: caller_ret,
            params: caller_params,
            variadic: caller_variadic,
        } = self.types.kind(body.fn_type())
        else {
            return fail("the function's type is not a function type");
        };
        let TypeKind::Function { ret, params, variadic } = self.types.kind(call.fn_ty) else {
            return fail("the call's type is not a function type");
        };
        let (caller_cc, caller_attrs) = self.caller();
        let caller_attr =
            |index: usize| caller_attrs.get(index).copied().unwrap_or(ParamAttrs::NONE);
        if variadic != caller_variadic {
            return fail("one of the function and the callee is variadic");
        }
        if ret != caller_ret {
            return fail("the function and the callee return different types");
        }
        if call.cc != caller_cc {
            return fail("the function and the call have different conventions");
        }
        // Variable arguments of the call itself may come before the
        // function's own, which a variadic function passes on (`...`):
        // LLVM's verifier accepts them (WP-064).
        // Next: `ret`, after an optional `bitcast` of the result; it
        // returns nothing, the result, or `undef` or `poison`.
        let end = body.blocks[block.index()].end;
        let is = |value: Value, raw: u32| matches!(value.kind(), ValueKind::Inst(def) if def.raw() == raw);
        let mut result = id;
        let mut next = id + 1;
        if next < end && body.insts[next as usize].op == Opcode::BitCast {
            if !is(body.insts[next as usize].ops[0], id) {
                return fail("the bitcast after it does not cast the result");
            }
            result = next;
            next += 1;
        }
        let returned = (next < end && body.insts[next as usize].op == Opcode::Ret)
            .then(|| body.insts[next as usize].ops[0]);
        let Some(returned) = returned else {
            return fail("a ret does not follow it");
        };
        let undef = returned.as_const().is_some_and(|constant| {
            matches!(self.module.consts.get(constant), Const::Undef(_) | Const::Poison(_))
        });
        if !(returned.is_none() || is(returned, result) || undef) {
            return fail("the ret after it does not return its result");
        }
        if matches!(call.cc, CallConv::TAIL | CallConv::SWIFT_TAIL) {
            let by_function =
                (0..caller_params.len()).any(|index| tailcc_refuses(caller_attr(index)));
            let by_call =
                (0..params.len()).any(|index| tailcc_refuses(self.call_attrs(call, Some(index))));
            if by_function || by_call {
                return fail("inalloca, inreg, swifterror, preallocated or byref with tailcc");
            }
            if caller_variadic {
                return fail("a tailcc function is variadic");
            }
            return Ok(());
        }
        if !intrinsic && params != caller_params {
            return fail("the function and the callee have different parameters");
        }
        for index in 0..caller_params.len() {
            if abi(caller_attr(index)) != abi(self.call_attrs(call, Some(index))) {
                let what =
                    format!("parameter {index} has different ABI attributes in the function");
                return fail(&what);
            }
        }
        Ok(())
    }
}
