//! Variadic functions with a body on x86-64 (`spec/interp.md` §5.12).
//!
//! A call of a variadic function lays its variable arguments out as the
//! System V psABI and `spec/abi-x64.md` §3, §9 say: a 176-byte register
//! save area (`rdi` .. `r9` at 0 .. 40, `xmm0` .. `xmm7` at 48 .. 160,
//! 16 bytes each) and the call's stack-argument area. Both live as long as
//! the callee's frame. `llvm.va_start` writes the 24-byte `va_list` from
//! where the named arguments stopped; the loads clang and kiln open-code
//! for `va_arg` then read those bytes like any others. Only the bytes a
//! caller is obliged to pass are written: the rest of a register or a
//! stack slot is unwritten, and reading it traps `uninitialized-load`.
//!
//! The rule is the subset of `spec/abi-x64.md` the interpreter needs,
//! written down again here rather than taken from `hod_x64::abi`: the
//! oracle does not share code with what it checks.

use hod_ir::{Arch, Body, CallSite, ParamAttrs, ParamMem, TypeId, TypeKind};

use super::{Class, Frame, Machine};
use crate::error::{Stop, Trap, invalid, unsupported};
use crate::memory::AllocKind;
use crate::value::{Val, sext};

/// A variadic call on a target other than x86-64, or an argument type the
/// model has no rule for (§5.12).
pub(super) const VARIADIC: &str = "call:variadic";
/// An argument or return type outside the model's subset (§5.12).
const VARIADIC_TYPE: &str = "call:variadic-type";
/// An argument attribute outside the model's subset (§5.12).
const VARIADIC_ATTR: &str = "call:variadic-attr";

/// Bytes of the register save area.
const SAVE_AREA: u64 = 176;
/// General argument registers, and vector argument registers.
const GPRS: u32 = 6;
const XMMS: u32 = 8;

/// What a variadic frame's `llvm.va_start` writes (§5.12).
#[derive(Clone, Copy, Debug)]
pub(super) struct VaArea {
    /// The register save area.
    save: u64,
    /// `overflow_arg_area`: the first variable argument on the stack.
    overflow: u64,
    gp_offset: u32,
    fp_offset: u32,
}

/// Where a piece of a variable argument goes.
#[derive(Clone, Copy, Debug)]
enum At {
    /// At this offset in the register save area.
    Save(u64),
    /// At this offset in the stack-argument area.
    Stack(u64),
}

#[derive(Clone, Debug)]
enum What {
    /// The value's bytes, as a store of it writes them.
    Value(Val),
    /// `size` bytes copied from the address (`byval`).
    Copy { from: u64, size: u64 },
}

/// The layout of one variadic call, worked out before its frame exists.
#[derive(Debug, Default)]
pub(super) struct VaCall {
    gpr_used: u32,
    xmm_used: u32,
    /// Where the named arguments' stack slots end, rounded up to 8.
    named_stack: u64,
    /// The end of the last stack slot.
    stack_size: u64,
    pieces: Vec<(At, What)>,
}

/// The argument-passing state of `spec/abi-x64.md` §3.1.
#[derive(Default)]
struct State {
    g: u32,
    v: u32,
    o: u64,
}

impl State {
    fn stack(&mut self, size: u64, align: u64) -> u64 {
        let at = self.o.next_multiple_of(align);
        self.o = at + size;
        at
    }

    /// One general piece: a register slot, or an 8-byte stack slot.
    fn general(&mut self) -> At {
        if self.g < GPRS {
            self.g += 1;
            At::Save(8 * u64::from(self.g - 1))
        } else {
            At::Stack(self.stack(8, 8))
        }
    }

    /// One vector piece; `wide` pieces take a 16-byte stack slot.
    fn vector(&mut self, wide: bool) -> At {
        if self.v < XMMS {
            self.v += 1;
            At::Save(48 + 16 * u64::from(self.v - 1))
        } else if wide {
            At::Stack(self.stack(16, 16))
        } else {
            At::Stack(self.stack(8, 8))
        }
    }
}

impl<'a> Machine<'_, 'a> {
    /// Whether variadic functions with a body are modelled for the
    /// module's target.
    pub(super) fn models_varargs(&self) -> bool {
        self.module.target().arch() == Arch::X86_64
    }

    /// The layout of a call of variadic `fn_ty` whose first arguments are
    /// the named ones (`named` types and attributes) and whose variable
    /// arguments are `varargs`.
    pub(super) fn va_call(
        &self,
        fn_ty: TypeId,
        named: &[(TypeId, ParamAttrs)],
        varargs: &[(TypeId, ParamAttrs, Val)],
    ) -> Result<VaCall, Stop> {
        let TypeKind::Function { ret, .. } = self.module.types.kind(fn_ty) else {
            return invalid("a variadic call without a function type");
        };
        // A return value in registers: no hidden first argument (§5.12).
        match self.class(ret)? {
            Class::Void
            | Class::Int(_)
            | Class::Ptr
            | Class::Float
            | Class::Double
            | Class::X86Fp80 => {}
            Class::Agg | Class::Vector(..) => return unsupported(VARIADIC_TYPE),
        }
        let mut state = State::default();
        let mut call = VaCall::default();
        for &(ty, attrs) in named {
            self.va_piece(&mut state, ty, attrs, None, &mut call)?;
        }
        call.gpr_used = state.g;
        call.xmm_used = state.v;
        call.named_stack = state.o.next_multiple_of(8);
        for (ty, attrs, val) in varargs {
            self.va_piece(&mut state, *ty, *attrs, Some(val), &mut call)?;
        }
        call.stack_size = state.o;
        Ok(call)
    }

    /// Assigns one argument; a variable one (`val`) also records what is
    /// written where.
    fn va_piece(
        &self,
        state: &mut State,
        ty: TypeId,
        attrs: ParamAttrs,
        val: Option<&Val>,
        call: &mut VaCall,
    ) -> Result<(), Stop> {
        let types = &self.module.types;
        if attrs.has(ParamAttrs::NEST) {
            return unsupported(VARIADIC_ATTR);
        }
        match attrs.mem() {
            ParamMem::ByVal => {
                let Some(pointee) = attrs.mem_type() else {
                    return invalid("byval without a type");
                };
                let align = attrs
                    .stack_align()
                    .or(attrs.align())
                    .unwrap_or_else(|| types.align_of(pointee))
                    .max(8);
                if align > 16 {
                    return unsupported(VARIADIC_ATTR);
                }
                let size = types.alloc_size_of(pointee);
                let at = state.o.next_multiple_of(align);
                state.o = at + size.max(8);
                if let Some(val) = val {
                    let Val::Ptr(from) = *val else {
                        return invalid("a byval argument that is not a pointer");
                    };
                    call.pieces.push((At::Stack(at), What::Copy { from, size }));
                }
                return Ok(());
            }
            ParamMem::InAlloca => return unsupported("attr:inalloca"),
            ParamMem::Preallocated => return unsupported("attr:preallocated"),
            _ => {}
        }
        if let TypeKind::Ptr { addr_space: addrspace } = types.kind(ty)
            && addrspace != 0
        {
            return unsupported(VARIADIC_TYPE);
        }
        let (at, written) = match self.class(ty)? {
            Class::Int(bits) if bits <= 64 => {
                let at = state.general();
                let written = match val {
                    Some(val) => Some(Self::extended(val, bits, attrs)?),
                    None => None,
                };
                (at, written)
            }
            Class::Ptr => (state.general(), val.cloned()),
            Class::Int(128) => {
                // §4.2: two registers if two are left, else 16 bytes on
                // the stack aligned to 16, leaving the registers.
                let at = if state.g + 2 <= GPRS {
                    state.g += 2;
                    At::Save(8 * u64::from(state.g - 2))
                } else {
                    At::Stack(state.stack(16, 16))
                };
                (at, val.cloned())
            }
            Class::Int(_) => return unsupported(VARIADIC_TYPE),
            Class::Float | Class::Double => (state.vector(false), val.cloned()),
            Class::X86Fp80 => (At::Stack(state.stack(16, 16)), val.cloned()),
            Class::Vector(..) => {
                // §4.3: a power-of-two vector of at most 16 bytes is one
                // vector piece; the other shapes are not modelled.
                let TypeKind::Vector { elem, len } = types.kind(ty) else {
                    return invalid("a vector class without a vector type");
                };
                let size = types.size_of(ty);
                let lane_ok = matches!(
                    types.kind(elem),
                    TypeKind::Int(8 | 16 | 32 | 64) | TypeKind::Float | TypeKind::Double
                ) || matches!(types.kind(elem), TypeKind::Ptr { addr_space: 0 });
                if !lane_ok || len < 2 || !len.is_power_of_two() || size > 16 {
                    return unsupported(VARIADIC_TYPE);
                }
                (state.vector(true), val.cloned())
            }
            Class::Agg | Class::Void => return unsupported(VARIADIC_TYPE),
        };
        if let Some(written) = written {
            call.pieces.push((at, What::Value(written)));
        }
        Ok(())
    }

    /// What the caller owes in the register or slot of integer `val` of
    /// `bits` bits with `attrs` (`spec/abi-x64.md` §7): its bytes, or
    /// those of its extension to 32 or 64 bits. A width that is not whole
    /// bytes leaves bits unspecified inside a byte, which memory cannot
    /// hold: refused.
    fn extended(val: &Val, bits: u32, attrs: ParamAttrs) -> Result<Val, Stop> {
        let Val::Int { value, .. } = *val else {
            return invalid("an integer argument that is not an integer");
        };
        let signed = attrs.has(ParamAttrs::SIGNEXT);
        let to = if !signed && !attrs.has(ParamAttrs::ZEROEXT) {
            bits
        } else if bits < 32 {
            32
        } else if bits <= 32 {
            bits
        } else {
            64
        };
        if to % 8 != 0 {
            return unsupported(VARIADIC_TYPE);
        }
        let value = if signed { sext(bits, value) as u128 } else { value };
        Ok(Val::int(to, value))
    }

    /// Allocates the register save area and the stack-argument area of a
    /// variadic frame and writes the variable arguments into them. The
    /// caller releases the frame's memory on an error.
    pub(super) fn lay_out_va(&mut self, call: &VaCall) -> Result<VaArea, Stop> {
        let save = self.mem.alloc(SAVE_AREA, 16, AllocKind::Stack)?;
        let stack = self.mem.alloc(call.stack_size.max(1), 16, AllocKind::Stack)?;
        for (at, what) in &call.pieces {
            let addr = match *at {
                At::Save(offset) => save + offset,
                At::Stack(offset) => stack + offset,
            };
            match what {
                What::Value(val) => self.store(val, addr, 1)?,
                What::Copy { from, size } => self.mem.copy((addr, 1), (*from, 1), *size)?,
            }
        }
        Ok(VaArea {
            save,
            overflow: stack + call.named_stack,
            gp_offset: 8 * call.gpr_used,
            fp_offset: 48 + 16 * call.xmm_used,
        })
    }

    /// `llvm.va_start`, `llvm.va_end` and `llvm.va_copy` (§5.12). `None`
    /// when the target has no model (the caller reports the intrinsic).
    pub(super) fn va_intrinsic(
        &mut self,
        f: &Frame<'a>,
        which: VaOp,
        call: CallSite<'a>,
    ) -> Result<Option<()>, Stop> {
        if !self.models_varargs() {
            return Ok(None);
        }
        let Some(&list) = call.args().first() else {
            return invalid("a va_ intrinsic without an argument");
        };
        let list = self.ptr(f, list)?;
        match which {
            VaOp::Start => {
                let Some(area) = f.va else {
                    return invalid("llvm.va_start in a function that is not variadic");
                };
                let mut bytes = [0u8; 24];
                bytes[0..4].copy_from_slice(&area.gp_offset.to_le_bytes());
                bytes[4..8].copy_from_slice(&area.fp_offset.to_le_bytes());
                bytes[8..16].copy_from_slice(&area.overflow.to_le_bytes());
                bytes[16..24].copy_from_slice(&area.save.to_le_bytes());
                self.mem.store(list, &bytes, 1)?;
            }
            VaOp::End => {
                // The list is no longer usable: its bytes are unwritten.
                let at = self.mem.check(list, 24, 1, true)?;
                self.mem.set_written(at, 24, false);
            }
            VaOp::Copy => {
                let Some(&src) = call.args().get(1) else {
                    return invalid("llvm.va_copy without a source");
                };
                let src = self.ptr(f, src)?;
                let apart = list.abs_diff(src) >= 24;
                if !apart && list != src {
                    return Err(Trap::OverlappingCopy.into());
                }
                self.mem.copy((list, 1), (src, 1), 24)?;
            }
        }
        Ok(Some(()))
    }

    /// The layout of entry function `body`'s own variable arguments: none.
    pub(super) fn entry_va(&self, body: &Body) -> Result<Option<VaCall>, Stop> {
        let fn_ty = body.fn_type();
        let TypeKind::Function { variadic: true, .. } = self.module.types.kind(fn_ty) else {
            return Ok(None);
        };
        if !self.models_varargs() {
            return Ok(None);
        }
        let named: Vec<(TypeId, ParamAttrs)> = body
            .arg_types(self.module)
            .iter()
            .enumerate()
            .map(|(index, &ty)| (ty, self.module.symbol(body.symbol()).param_attrs(index)))
            .collect();
        self.va_call(fn_ty, &named, &[]).map(Some)
    }
}

/// The three `va_` intrinsics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VaOp {
    Start,
    End,
    Copy,
}
