//! Argument assignment: `spec/codegen.md` §7.2.
//!
//! A target's `assign` function maps a [`Sig`] to an [`Assignment`]: where
//! each byte of each argument and of the return value travels. The same
//! answer serves a call (outgoing placement) and a function's entry
//! (incoming locations). The rules are the target's, in
//! `spec/abi-<target>.md`.

use hod_ir::{CallConv, ParamAttrs, TypeId};

use crate::reg::PReg;

/// What is being called or defined, as far as passing goes.
#[derive(Clone, Copy, Debug)]
pub struct Sig<'a> {
    /// The calling convention.
    pub cc: CallConv,
    /// The return type; `TypeId::VOID` for none.
    pub ret: TypeId,
    /// The attributes of the return value.
    pub ret_attrs: ParamAttrs,
    /// The types of the arguments passed: for a call through a variadic
    /// type, the variable ones included.
    pub params: &'a [TypeId],
    /// The attributes of the first `param_attrs.len()` arguments; the rest
    /// have none.
    pub param_attrs: &'a [ParamAttrs],
    /// The number of named parameters: `params.len()` unless `variadic`.
    pub fixed: u32,
    /// Whether the function type is variadic.
    pub variadic: bool,
}

impl Sig<'_> {
    /// The attributes of argument `index`.
    pub fn attrs(&self, index: usize) -> ParamAttrs {
        self.param_attrs.get(index).copied().unwrap_or(ParamAttrs::NONE)
    }
}

/// Where a part travels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    /// In a register, in its low bytes.
    Reg(PReg),
    /// In the stack-argument area, at this byte offset from its lowest
    /// address (the stack pointer at the call instruction).
    Stack(u32),
}

/// What the bytes of a part are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartKind {
    /// Bytes `offset..offset + size` of the argument's value (of its
    /// in-memory image, `spec/ir.md` §4.3, for a `Mem`-class value).
    Value,
    /// Bytes `offset..offset + size` of the memory the argument points
    /// to: a `byval` argument, which is copied into the stack-argument
    /// area. In the callee the argument's value is the address of that
    /// copy.
    Pointee,
}

/// One contiguous piece of an argument or of the return value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Part {
    /// The argument's index in `Sig::params`; 0 for a part of the return
    /// value.
    pub arg: u32,
    /// What the bytes are.
    pub kind: PartKind,
    /// The first byte of the piece.
    pub offset: u32,
    /// Its size in bytes: at most the register's size for a register.
    pub size: u32,
    /// Where it travels.
    pub place: Place,
}

/// How a return value too large for the return registers travels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetMemory {
    /// Where the caller passes the address of the buffer the callee
    /// writes the value to.
    pub address: Place,
    /// The register in which the callee hands that address back, on a
    /// target whose convention requires it.
    pub returned_in: Option<PReg>,
}

/// What `va_start` needs to know about the named parameters.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct VaInfo {
    /// How many general argument registers the named parameters used.
    pub gpr_used: u8,
    /// How many vector argument registers the named parameters used.
    pub fpr_used: u8,
    /// The offset in the stack-argument area of the first variable
    /// argument passed on the stack.
    pub stack_offset: u32,
}

/// The answer of `assign`. Reused between calls: `clear` keeps the
/// allocation.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct Assignment {
    /// The parts of the arguments, in argument order and, within an
    /// argument, in offset order. An argument that takes no space (an
    /// empty aggregate) has no part.
    pub args: Vec<Part>,
    /// The parts of the return value, in offset order; empty for `void`
    /// and when `ret_memory` is set.
    pub ret: Vec<Part>,
    /// Set when the return value travels in memory although the signature
    /// has no `sret` parameter.
    pub ret_memory: Option<RetMemory>,
    /// The register in which a function with an `sret` parameter hands
    /// that parameter back, on a target whose convention requires it.
    pub sret_returned_in: Option<PReg>,
    /// The size of the stack-argument area in bytes, not rounded to the
    /// stack alignment.
    pub stack_size: u32,
    /// For a variadic signature.
    pub va: VaInfo,
    /// How many vector registers the arguments use, for a convention that
    /// passes that count to a variadic callee (x86-64's `al`).
    pub vector_regs_used: u8,
}

impl Assignment {
    /// Empties the answer, keeping its buffers.
    pub fn clear(&mut self) {
        self.args.clear();
        self.ret.clear();
        self.ret_memory = None;
        self.sret_returned_in = None;
        self.stack_size = 0;
        self.va = VaInfo::default();
        self.vector_regs_used = 0;
    }
}

/// A signature a target cannot pass, under the stable name the coverage
/// log reports (`abi:<what>`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AbiUnsupported(pub &'static str);
