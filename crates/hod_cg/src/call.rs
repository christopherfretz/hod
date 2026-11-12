//! The call sequence: `spec/codegen.md` §4.4, §5.3, §7.1 to §7.3.
//!
//! One engine places the arguments of every call a function makes: an IR
//! `call` or `invoke`, a `musttail` call, and the runtime-function calls
//! that lowerings of other instructions make. It is built on the
//! target's `assign` and its glue; the target supplies the call
//! instruction itself (`Target::emit_call`).

use hod_ir::{
    AttrConflict, BlockId, CallConv, Inst, Opcode, ParamAttrs, SymbolKind, TailKind, TypeId,
    TypeKind, Value, ValueKind, call_arg_attrs, call_ret_attrs,
};

use crate::abi::{AbiUnsupported, Assignment, LaneLoc, Part, PartKind, Place, Sig};
use crate::cx::{FnCx, TailExit};
use crate::error::{CALL_MUSTTAIL_RET_MEMORY, CALL_PART_CLASS, CALL_PART_LANES, CgError, CgResult};
use crate::frame::FrameRef;
use crate::loc::{Class, Key, Loc};
use crate::record::{CallSiteRow, NO_PAD};
use crate::reg::{PReg, RegSet};
use crate::sym::{RuntimeFn, SymRef};
use crate::target::{CallEmit, CallTarget, Target};

/// Whether `attrs` asks for an extension (`zeroext` or `signext`).
fn extends(attrs: ParamAttrs) -> bool {
    attrs.has(ParamAttrs::ZEROEXT) || attrs.has(ParamAttrs::SIGNEXT)
}

/// Whether a part's elements travel as lanes (§7.2, WP-082).
pub(crate) fn is_lanes(kind: PartKind) -> bool {
    matches!(kind, PartKind::Lanes { .. })
}

/// An operand of a runtime-function call (§7.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallArg {
    /// A value of the body, as an IR call passes it. A register value
    /// passed this way is consumed by the call.
    Value(Value),
    /// What a temporary register holds ([`FnCx::temp`]): something the
    /// lowering computed. It must be a pool register.
    Reg(PReg),
    /// The address of a frame slot.
    Addr(FrameRef),
    /// A `Mem`-class value that is in a frame slot.
    Mem(FrameRef),
}

/// The arguments of the call being placed.
#[derive(Clone, Copy)]
enum Args<'x> {
    Values(&'x [Value]),
    Custom(&'x [CallArg]),
}

impl Args<'_> {
    fn len(&self) -> usize {
        match self {
            Args::Values(values) => values.len(),
            Args::Custom(args) => args.len(),
        }
    }

    fn get(&self, index: usize) -> Option<CallArg> {
        match self {
            Args::Values(values) => values.get(index).map(|&value| CallArg::Value(value)),
            Args::Custom(args) => args.get(index).copied(),
        }
    }
}

/// What is called.
#[derive(Clone, Copy)]
enum Callee {
    Sym(SymRef),
    Value(Value),
}

/// A call, as far as the engine needs to know it. The parameter types
/// and attributes are in the scratch lists.
#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools, reason = "independent facts of one call site")]
struct CallDesc {
    cc: CallConv,
    ret: TypeId,
    ret_attrs: ParamAttrs,
    fixed: u32,
    variadic: bool,
    callee: Callee,
    noreturn: bool,
    returns_twice: bool,
    musttail: bool,
    /// The landing-pad block of an `invoke`.
    unwind: Option<BlockId>,
    /// Where a `Mem`-class result is written.
    ret_mem: Option<FrameRef>,
}

/// What the engine did.
#[derive(Clone, Copy)]
struct Called {
    /// The register a `Gpr`- or `Fpr`-class result is in, as a pinned
    /// temporary.
    reg: Option<PReg>,
    /// How far the stack pointer was lowered around the call.
    area: u32,
    /// The index of the call's row in the chunk's call-site table.
    row: Option<usize>,
}

impl<'a, T: Target> FnCx<'a, T> {
    /// Runs the call sequence of §7.1 for `inst`, a `call` or `invoke`
    /// of a value (not of an intrinsic: those are the lowering's):
    /// displaces registers, places the arguments by the target's
    /// `assign`, emits the call through `Target::emit_call`, records the
    /// call site, takes the result, and for an `invoke` writes the
    /// destinations' incoming slots, branches to the normal destination
    /// and emits the pad stub. A `musttail` call becomes a branch to a
    /// tail exit, and the rest of its block is not emitted.
    ///
    /// # Errors
    /// `Unsupported` under the name the target's `assign` gives, or one
    /// of the `call:` names of this crate; `Internal` for a call
    /// `classify` should have rejected.
    pub fn call(&mut self, inst: Inst<'a>) -> CgResult<()> {
        let Some(site) = inst.call() else {
            return self.bug("call of an instruction that is no call");
        };
        if site.intrinsic().is_some() || inst.opcode() == Opcode::CallBr {
            return self.bug("the call sequence was asked for an intrinsic or a callbr");
        }
        let TypeKind::Function { ret, params, variadic } = self.types().kind(site.fn_type()) else {
            return self.bug("a call through a type that is no function type");
        };
        let args = site.args();
        if args.len() < params.len() || (!variadic && args.len() != params.len()) {
            return self.bug("a call whose arguments do not match its function type");
        }
        let unwind = site.unwind_dest();
        let normal = site.normal_dest();
        let callee = site.callee_value();
        // §7.1 step 1.
        if let Some(pad) = unwind {
            self.phi_stores(pad)?;
        }
        // A deferred argument is emitted before anything is displaced:
        // its lowering needs registers.
        for &value in args.iter().chain(std::iter::once(&callee)) {
            if self.deferred(value).is_some() {
                self.use_reg(value)?;
            }
        }
        // The signature, with the attributes the call passes with
        // (§7.1): its own and, for a direct call through the callee's
        // own function type, the declaration's.
        let conflict = |_| CgError::unsupported(AttrConflict::NAME);
        let ret_attrs = call_ret_attrs(self.module, &site).map_err(conflict)?;
        self.s.sig_params.clear();
        self.s.sig_attrs.clear();
        for (index, &value) in args.iter().enumerate() {
            let ty = match params.get(index) {
                Some(&ty) => ty,
                None => self.body.value_type(self.module, value),
            };
            let attrs = call_arg_attrs(self.module, &site, index).map_err(conflict)?;
            self.s.sig_params.push(ty);
            self.s.sig_attrs.push(attrs);
        }
        let musttail = site.tail() == TailKind::MustTail;
        let ret_mem = if ret != TypeId::VOID
            && !musttail
            && self.target.class_of(self.types(), ret) == Class::Mem
        {
            Some(self.def_mem(inst)?)
        } else {
            None
        };
        let desc = CallDesc {
            cc: site.cc(),
            ret,
            ret_attrs,
            fixed: params.len() as u32,
            variadic,
            callee: Callee::Value(callee),
            noreturn: site.is_noreturn(),
            returns_twice: site.returns_twice(),
            musttail,
            unwind,
            ret_mem,
        };
        let done = self.run_call(&desc, Args::Values(args))?;
        if musttail {
            return Ok(());
        }
        if let Some(reg) = done.reg {
            self.finish(inst, reg)?;
        }
        // WP-084: an `invoke`'s result is read on the normal edge, after
        // the branch: the bits above its odd integer leaves are cleared
        // here, before the phi stores (a call's are cleared by the target).
        if let (Some(_), Some(home)) = (unwind, ret_mem) {
            self.clear_odd_leaves(ret, home);
        }
        // §7.1 step 6.
        if let (Some(normal), Some(pad)) = (normal, unwind) {
            let t = self.target;
            self.phi_stores(normal)?;
            let stub = done.area != 0;
            if stub || !self.falls_through(normal) {
                t.jump(self, self.block_label(normal))?;
            }
            if stub {
                // §5.3: the unwinder enters the pad with the stack
                // pointer as it was at the call.
                let at = self.offset();
                t.adjust_sp(self, i64::from(done.area));
                t.jump(self, self.block_label(pad))?;
                if let Some(row) = done.row.and_then(|row| self.out.call_sites.get_mut(row)) {
                    row.landing = at;
                }
            }
        }
        Ok(())
    }

    /// Calls runtime function `func` (§7.3) with `args`, by the same
    /// sequence as an IR call: registers are displaced, the call site
    /// is recorded. A result of class `Gpr` or `Fpr` is returned in its
    /// return register, as a pinned temporary that
    /// [`FnCx::finish`] accepts; a result of class `Mem` is written to
    /// `ret_mem`.
    ///
    /// Every temporary of the lowering is gone afterwards, and so is
    /// every register [`FnCx::use_reg`] returned: pass what the call
    /// needs as arguments, and ask again for what is needed after it.
    ///
    /// # Errors
    /// As [`FnCx::call`]; `Internal` for an argument list that does not
    /// match the function, or a `Mem`-class result without `ret_mem`.
    pub fn call_runtime(
        &mut self,
        func: RuntimeFn,
        args: &[CallArg],
        ret_mem: Option<FrameRef>,
    ) -> CgResult<Option<PReg>> {
        if args.len() != func.params().len() {
            return self.bug("a runtime call with the wrong number of arguments");
        }
        for arg in args {
            if let CallArg::Value(value) = *arg
                && self.deferred(value).is_some()
            {
                self.use_reg(value)?;
            }
        }
        self.s.sig_params.clear();
        self.s.sig_params.extend_from_slice(func.params());
        self.s.sig_attrs.clear();
        let desc = CallDesc {
            cc: CallConv::C,
            ret: func.ret(),
            ret_attrs: ParamAttrs::NONE,
            fixed: args.len() as u32,
            variadic: false,
            callee: Callee::Sym(SymRef::Runtime(func)),
            noreturn: false,
            returns_twice: false,
            musttail: false,
            unwind: None,
            ret_mem,
        };
        Ok(self.run_call(&desc, Args::Custom(args))?.reg)
    }

    /// Records the call site of a call instruction a lowering emitted
    /// itself (§7.1: the call inside a TLS access sequence): `len` bytes
    /// at offset `start`. The lowering has displaced what the sequence
    /// clobbers.
    pub fn record_call_site(&mut self, start: u32, len: u32) {
        self.push_call_site(start, len, None, false);
    }

    /// Whether a call of `target` cannot unwind, as `llc` decides it
    /// (`EHStreamer::callToNoUnwindFunction`): a direct call of a
    /// function declared `nounwind`. A runtime function, an indirect
    /// callee, an alias and a call-site `nounwind` do not count.
    fn callee_is_nounwind(&self, target: CallTarget) -> bool {
        let CallTarget::Sym(SymRef::Module(sym)) = target else { return false };
        match self.module.symbol(sym).kind() {
            SymbolKind::Function { attrs, .. } => {
                self.module.fn_attrs(attrs).has(hod_ir::FnAttrs::NOUNWIND)
            }
            _ => false,
        }
    }

    fn push_call_site(
        &mut self,
        start: u32,
        len: u32,
        unwind: Option<BlockId>,
        nounwind: bool,
    ) -> Option<usize> {
        // §9.4: recorded only for a function that has landing pads.
        if !self.has_pads {
            return None;
        }
        self.out.call_sites.push(CallSiteRow { start, len, pad: NO_PAD, landing: 0, nounwind });
        self.s.site_pads.push(unwind.map_or(u32::MAX, BlockId::raw));
        Some(self.out.call_sites.len() - 1)
    }

    /// The function's own assignment (§7.2): where its arguments arrive
    /// and its return value leaves.
    #[inline]
    pub fn fn_assignment(&self) -> &Assignment {
        &self.s.fn_asg
    }

    /// The number of tail exits `end_function` emits after the epilogue
    /// (§5.4): one per `musttail` call.
    #[inline]
    pub fn tail_exit_count(&self) -> usize {
        self.s.tail_exits.len()
    }

    /// Tail exit `index`: the label to bind at its start, and what its
    /// teardown is followed by a branch to.
    ///
    /// # Panics
    /// If `index` is not below [`FnCx::tail_exit_count`].
    #[inline]
    pub fn tail_exit(&self, index: usize) -> TailExit {
        self.s.tail_exits[index]
    }

    /// Places `value` where the function returns its value (§5.4): what
    /// `Ret` does before it branches to the epilogue. A value of class
    /// `Gpr` or `Fpr` goes to its return register, extended as the ABI
    /// asks of the producer; a `Mem`-class value that travels in
    /// registers is loaded part by part.
    ///
    /// # Errors
    /// `Internal` when the function returns in memory
    /// (`Assignment::ret_memory`): where the buffer's address is kept is
    /// the target's, and so is that copy.
    pub fn place_return(&mut self, value: Value) -> CgResult<()> {
        let t = self.target;
        if self.s.fn_asg.ret_memory.is_some() {
            return self.bug("place_return in a function that returns in memory");
        }
        let class = self.class_of(value);
        let ret_ty = self.body.ret_type(self.module);
        let source = if class == Class::Mem { Some(self.use_mem(value)?) } else { None };
        // WP-082: a mask whose elements travel as lanes is converted from
        // its bits, which wait in a scratch register.
        let bits = self.scratch_reg(Class::Gpr);
        if source.is_none() && self.s.fn_asg.ret.iter().any(|part| is_lanes(part.kind)) {
            self.put(value, bits)?;
        }
        for index in 0..self.s.fn_asg.ret.len() {
            let part = self.s.fn_asg.ret[index];
            let Place::Reg(reg) = part.place else {
                return self.bug("a part of the return value on the stack");
            };
            if is_lanes(part.kind) {
                let from = match source {
                    Some(slot) => LaneLoc::Frame(slot.offset_by(part.offset)),
                    None => LaneLoc::Reg(bits),
                };
                t.lanes_out(self, &part, from, LaneLoc::Reg(reg))?;
                continue;
            }
            if let Some(slot) = source {
                t.load(self, reg, slot.offset_by(part.offset), part.size);
                self.extend_last_piece(ret_ty, self.ret_attrs, &part, slot, None);
            } else {
                self.check_class(reg, class)?;
                self.put(value, reg)?;
                if class == Class::Gpr {
                    t.extend_arg(self, reg, ret_ty, self.ret_attrs);
                }
            }
        }
        Ok(())
    }

    /// A part of a `Gpr`- or `Fpr`-class value can only travel in a
    /// register of its class: glue cannot move between classes.
    fn check_class(&self, reg: PReg, class: Class) -> CgResult<()> {
        let of_class = if class == Class::Fpr { self.ri.fpr } else { self.ri.gpr };
        if of_class.contains(reg) { Ok(()) } else { Err(CgError::unsupported(CALL_PART_CLASS)) }
    }

    /// Runs the engine with the scratch lists taken out of the scratch
    /// state, so that the glue it calls can borrow the context.
    fn run_call(&mut self, call: &CallDesc, args: Args<'_>) -> CgResult<Called> {
        let mut asg = std::mem::take(&mut self.s.asg);
        let params = std::mem::take(&mut self.s.sig_params);
        let attrs = std::mem::take(&mut self.s.sig_attrs);
        let sig = Sig {
            cc: call.cc,
            ret: call.ret,
            ret_attrs: call.ret_attrs,
            params: &params,
            param_attrs: &attrs,
            fixed: call.fixed,
            variadic: call.variadic,
        };
        asg.clear();
        let result = match self.target.assign(self.types(), &sig, &mut asg) {
            Ok(()) => self.place_and_call(call, args, &sig, &asg),
            Err(AbiUnsupported(name)) => Err(CgError::unsupported(name)),
        };
        self.s.asg = asg;
        self.s.sig_params = params;
        self.s.sig_attrs = attrs;
        result
    }

    #[allow(clippy::too_many_lines)]
    fn place_and_call(
        &mut self,
        call: &CallDesc,
        args: Args<'_>,
        sig: &Sig<'_>,
        asg: &Assignment,
    ) -> CgResult<Called> {
        let t = self.target;
        if args.len() != sig.params.len() {
            return self.bug("a call whose arguments do not match its signature");
        }
        if call.musttail && asg.ret_memory.is_some() {
            return Err(CgError::unsupported(CALL_MUSTTAIL_RET_MEMORY));
        }
        // §4.3: a spill slot occupied at this call is read again when
        // the call returns a second time.
        if call.returns_twice {
            self.s.frame.keep_spills();
        }
        // §4.4: what stays in registers are operands of this call in
        // pool registers. Argument placement writes none of those.
        let mut keep = RegSet::EMPTY;
        for index in 0..args.len() {
            match args.get(index) {
                Some(CallArg::Value(value)) => keep = keep.union(self.kept(value)?),
                Some(CallArg::Reg(reg)) if !self.pool.contains(reg) => {
                    return self.bug("a call argument in a register that is not a pool register");
                }
                _ => {}
            }
        }
        if let Callee::Value(value) = call.callee {
            keep = keep.union(self.kept(value)?);
        }
        self.s.pinned = RegSet::EMPTY;
        for reg in self.s.occupied.difference(keep).iter() {
            self.displace(reg)?;
        }

        // §5.3, §7.1: where the stack arguments are written.
        let area = asg.stack_size.next_multiple_of(T::STACK_ALIGN);
        let stack = if asg.stack_size == 0 {
            None
        } else if call.musttail {
            // They overwrite this function's own incoming arguments,
            // which may be the homes of the values being passed: they
            // are staged in the local area first.
            if asg.stack_size > self.s.fn_asg.stack_size.next_multiple_of(T::STACK_ALIGN) {
                return self.bug("a musttail call with more stack arguments than its caller has");
            }
            Some(self.s.frame.alloc_temp(u64::from(asg.stack_size))?)
        } else {
            t.adjust_sp(self, -i64::from(area));
            Some(FrameRef::Outgoing(0))
        };

        // Stack arguments, then register arguments.
        let mut mem_source = (u32::MAX, FrameRef::Local(0));
        for on_stack in [true, false] {
            for part in &asg.args {
                if matches!(part.place, Place::Stack(_)) != on_stack {
                    continue;
                }
                let Some(arg) = args.get(part.arg as usize) else {
                    return self.bug("a part of an argument the call does not have");
                };
                let ty = sig.params[part.arg as usize];
                let attrs = sig.attrs(part.arg as usize);
                self.place_part(arg, ty, attrs, part, stack, &mut mem_source)?;
            }
        }
        // WP-082: a register value returned in memory (a wide mask) is
        // received in a buffer of this instruction's own.
        let ret_buffer = match (asg.ret_memory, call.ret_mem) {
            (Some(_), None)
                if call.ret != TypeId::VOID && t.class_of(self.types(), call.ret) != Class::Mem =>
            {
                Some(self.s.frame.alloc_temp(self.types().alloc_size_of(call.ret))?)
            }
            (_, buffer) => buffer,
        };
        if let Some(memory) = asg.ret_memory {
            let Some(buffer) = ret_buffer else {
                return self.bug("a call that returns in memory without a place for the result");
            };
            match (memory.address, stack) {
                (Place::Reg(reg), _) => t.frame_addr(self, reg, buffer),
                (Place::Stack(offset), Some(base)) => {
                    let scratch = self.scratch_reg(Class::Gpr);
                    t.frame_addr(self, scratch, buffer);
                    t.store(self, scratch, base.offset_by(offset), self.reg_bytes(Class::Gpr));
                }
                (Place::Stack(_), None) => {
                    return self.bug("a stack part in a call without a stack-argument area");
                }
            }
        }
        if call.musttail
            && let Some(staging) = stack
        {
            t.copy(self, FrameRef::Incoming(0), staging, asg.stack_size);
        }
        // An indirect callee's address goes into its scratch register
        // last: glue is done with the scratch registers.
        let target = match call.callee {
            Callee::Sym(sym) => CallTarget::Sym(sym),
            Callee::Value(value) => match value.kind() {
                ValueKind::Sym(sym) => CallTarget::Sym(SymRef::Module(sym)),
                ValueKind::Asm(_) | ValueKind::None => {
                    return self.bug("a call of something that is no value");
                }
                _ => {
                    let reg = self.ri.callee;
                    self.put(value, reg)?;
                    CallTarget::Reg(reg)
                }
            },
        };
        // The operands are consumed, and no temporary outlives a call.
        for index in 0..args.len() {
            if let Some(CallArg::Value(value)) = args.get(index) {
                self.release(value);
            }
        }
        if let Callee::Value(value) = call.callee {
            self.release(value);
        }
        self.s.temps = RegSet::EMPTY;
        self.s.pinned = RegSet::EMPTY;

        if call.musttail {
            let label = self.new_label();
            self.s.tail_exits.push(TailExit { label, target });
            t.jump(self, label)?;
            self.skip_rest = true;
            return Ok(Called { reg: None, area: 0, row: None });
        }

        let start = self.offset();
        let emit =
            CallEmit { target, variadic: call.variadic, vector_regs_used: asg.vector_regs_used };
        t.emit_call(self, &emit)?;
        let len = self.offset() - start;
        let nounwind = self.callee_is_nounwind(target);
        let row = self.push_call_site(start, len, call.unwind, nounwind);
        if call.noreturn {
            self.trap();
        }
        if stack.is_some() {
            t.adjust_sp(self, i64::from(area));
        }

        // The result (§4.4): left in its return register, or copied
        // into the result's home.
        let mut reg = None;
        let class = t.class_of(self.types(), call.ret);
        if call.ret != TypeId::VOID
            && class != Class::Mem
            && asg.ret_memory.is_some()
            && let Some(buffer) = ret_buffer
        {
            // WP-082: the image the callee wrote, in a temporary.
            let to = self.temp(class)?;
            let bytes = self.types().alloc_size_of(call.ret).min(u64::from(self.reg_bytes(class)));
            t.load(self, to, buffer, bytes as u32);
            reg = Some(to);
        } else if call.ret != TypeId::VOID && asg.ret_memory.is_none() {
            if class == Class::Mem {
                let Some(buffer) = call.ret_mem else {
                    return self.bug("a call with a result of class Mem and no place for it");
                };
                for part in &asg.ret {
                    let Place::Reg(from) = part.place else {
                        return self.bug("a part of a return value on the stack");
                    };
                    let to = buffer.offset_by(part.offset);
                    if is_lanes(part.kind) {
                        t.lanes_in(self, part, LaneLoc::Reg(from), LaneLoc::Frame(to))?;
                    } else {
                        t.store(self, from, to, part.size);
                    }
                }
            } else if asg.ret.iter().any(|part| is_lanes(part.kind)) {
                // WP-082: a mask whose elements return as lanes: their
                // bits are gathered in a scratch register, then moved to
                // a temporary.
                let bits = self.scratch_reg(Class::Gpr);
                for part in &asg.ret {
                    let Place::Reg(from) = part.place else {
                        return self.bug("a part of a return value on the stack");
                    };
                    t.lanes_in(self, part, LaneLoc::Reg(from), LaneLoc::Reg(bits))?;
                }
                let to = self.temp(class)?;
                t.mov(self, to, bits);
                reg = Some(to);
            } else {
                let [Part { place: Place::Reg(from), .. }] = asg.ret[..] else {
                    return self.bug("a scalar return value that is not in one register");
                };
                self.check_class(from, class)?;
                self.s.temps = self.s.temps.with(from);
                self.s.pinned = self.s.pinned.with(from);
                reg = Some(from);
            }
        }
        Ok(Called { reg, area: if stack.is_some() { area } else { 0 }, row })
    }

    /// The register `value` may stay in across argument placement: the
    /// pool register it rests in, if it rests in one.
    fn kept(&mut self, value: Value) -> CgResult<RegSet> {
        if Key::of(value).is_some()
            && let Loc::Reg(reg) = self.loc(value)?
            && self.pool.contains(reg)
        {
            return Ok(RegSet::EMPTY.with(reg));
        }
        Ok(RegSet::EMPTY)
    }

    /// Places one part of one argument: in its register, or at its
    /// offset of the stack-argument area `stack`.
    fn place_part(
        &mut self,
        arg: CallArg,
        ty: TypeId,
        attrs: ParamAttrs,
        part: &Part,
        stack: Option<FrameRef>,
        mem_source: &mut (u32, FrameRef),
    ) -> CgResult<()> {
        let t = self.target;
        let to_stack = match (part.place, stack) {
            (Place::Reg(_), _) => None,
            (Place::Stack(offset), Some(base)) => Some(base.offset_by(offset)),
            (Place::Stack(_), None) => {
                return self.bug("a stack part in a call without a stack-argument area");
            }
        };
        let class = t.class_of(self.types(), ty);

        // The pointee of a `byval` argument: copied from where the
        // pointer points.
        if part.kind == PartKind::Pointee {
            let (CallArg::Value(pointer), Some(dst)) = (arg, to_stack) else {
                return Err(CgError::unsupported(crate::error::CALL_BYVAL_PLACE));
            };
            let scratch = self.scratch_reg(Class::Gpr);
            self.put(pointer, scratch)?;
            t.copy_from_ptr(self, dst, scratch, part.offset, part.size);
            return Ok(());
        }

        // WP-082: elements widened to lanes, moved by the target.
        if is_lanes(part.kind) {
            let to = match (part.place, to_stack) {
                (Place::Reg(reg), _) => LaneLoc::Reg(reg),
                (_, Some(dst)) => LaneLoc::Frame(dst),
                (Place::Stack(_), None) => {
                    return self.bug("a stack part in a call without a stack-argument area");
                }
            };
            let from = match arg {
                CallArg::Mem(slot) => LaneLoc::Frame(slot.offset_by(part.offset)),
                CallArg::Value(value) if class == Class::Mem => {
                    if mem_source.0 != part.arg {
                        *mem_source = (part.arg, self.use_mem(value)?);
                    }
                    LaneLoc::Frame(mem_source.1.offset_by(part.offset))
                }
                CallArg::Value(value) if class == Class::Gpr => {
                    let bits = self.scratch_reg(Class::Gpr);
                    self.put(value, bits)?;
                    LaneLoc::Reg(bits)
                }
                _ => return Err(CgError::unsupported(CALL_PART_LANES)),
            };
            return t.lanes_out(self, part, from, to);
        }

        // A value that is in memory: its bytes are moved from there.
        let in_frame = match arg {
            CallArg::Mem(slot) => Some(slot),
            CallArg::Value(value) if class == Class::Mem => {
                if mem_source.0 != part.arg {
                    *mem_source = (part.arg, self.use_mem(value)?);
                }
                Some(mem_source.1)
            }
            _ => None,
        };
        if let Some(slot) = in_frame {
            let from = slot.offset_by(part.offset);
            match (part.place, to_stack) {
                (_, Some(dst)) => t.copy(self, dst, from, part.size),
                (Place::Reg(reg), None) => t.load(self, reg, from, part.size),
                (Place::Stack(_), None) => {}
            }
            self.extend_last_piece(ty, attrs, part, slot, to_stack);
            return Ok(());
        }

        // A value of class `Gpr` or `Fpr`: through its argument
        // register, or through a scratch register onto the stack.
        let reg = match part.place {
            Place::Reg(reg) => reg,
            Place::Stack(_) => self.scratch_reg(class),
        };
        self.check_class(reg, class)?;
        match arg {
            CallArg::Value(value) => self.put(value, reg)?,
            CallArg::Reg(from) => {
                if from != reg {
                    t.mov(self, reg, from);
                }
            }
            CallArg::Addr(slot) => t.frame_addr(self, reg, slot),
            CallArg::Mem(_) => {}
        }
        if class == Class::Gpr && !matches!(arg, CallArg::Addr(_)) {
            t.extend_arg(self, reg, ty, attrs);
        }
        if let Some(dst) = to_stack {
            // §7.1: an extended integer fills its stack slot, as far as
            // the extension reaches (LLVM's callers store an `i7 zeroext`
            // as 32 bits, an `i33 signext` as 64).
            let size =
                if class == Class::Gpr && extends(attrs) && self.types().int_bits(ty).is_some() {
                    part.size.max(self.reg_bytes(Class::Gpr))
                } else {
                    part.size
                };
            t.store(self, reg, dst, size);
        }
        Ok(())
    }

    /// §2.2: clears the bits above every integer leaf of `ty` whose width
    /// is not a multiple of 8 in the image at `at`, a `Mem`-class value's
    /// home that an incoming argument's parts were written to. The ABI
    /// promises nothing above a leaf (`abi-<target>.md`); hod's memory
    /// holds zeros there. A leaf (its last piece, for a leaf wider than a
    /// register) is loaded into a scratch register, extended by zero as
    /// [`Target::extend_arg`] extends a `zeroext` argument, and stored
    /// back with its own size. Elements of vectors are not leaves here.
    pub(crate) fn clear_odd_leaves(&mut self, ty: TypeId, at: FrameRef) {
        if !self.has_odd_leaf(ty) {
            return;
        }
        match self.types().kind(ty) {
            TypeKind::Struct { fields, .. } => {
                let fields = fields.to_vec();
                let offsets =
                    self.types().field_offsets(ty).map(<[u64]>::to_vec).unwrap_or_default();
                for (field, offset) in fields.into_iter().zip(offsets) {
                    self.clear_odd_leaves(field, at.offset_by(offset as u32));
                }
            }
            TypeKind::Array { elem, len } => {
                let stride = self.types().alloc_size_of(elem) as u32;
                for index in 0..len as u32 {
                    self.clear_odd_leaves(elem, at.offset_by(index * stride));
                }
            }
            TypeKind::Int(bits) => {
                let t = self.target;
                let reg_bytes = self.reg_bytes(Class::Gpr);
                let last = (bits - 1) / (8 * reg_bytes) * reg_bytes;
                let size = bits.div_ceil(8) - last;
                let scratch = self.scratch_reg(Class::Gpr);
                t.load(self, scratch, at.offset_by(last), size);
                let zeroext =
                    ParamAttrs::NONE.with(ParamAttrs::ZEROEXT).unwrap_or(ParamAttrs::NONE);
                t.extend_arg(self, scratch, ty, zeroext);
                t.store(self, scratch, at.offset_by(last), size);
            }
            _ => {}
        }
    }

    /// Whether `ty` has an integer leaf of a width that is not a multiple
    /// of 8 (vectors' elements are not leaves).
    fn has_odd_leaf(&self, ty: TypeId) -> bool {
        match self.types().kind(ty) {
            TypeKind::Struct { fields, .. } => fields.iter().any(|&field| self.has_odd_leaf(field)),
            TypeKind::Array { elem, len } => len > 0 && self.has_odd_leaf(elem),
            TypeKind::Int(bits) => bits % 8 != 0,
            _ => false,
        }
    }

    /// §7.1: the last piece of an integer wider than a register, a
    /// `Mem`-class value whose image is in `slot`, brought to the form
    /// the ABI requires of its producer once `part` has been placed: in
    /// its register by [`Target::extend_arg`] (which decides what that
    /// form is), or on the stack through a scratch register and stored a
    /// whole register wide. Nothing for other parts and types, and on
    /// the stack nothing without `zeroext` or `signext`.
    fn extend_last_piece(
        &mut self,
        ty: TypeId,
        attrs: ParamAttrs,
        part: &Part,
        slot: FrameRef,
        to_stack: Option<FrameRef>,
    ) {
        let t = self.target;
        let reg_bytes = self.reg_bytes(Class::Gpr);
        let Some(bits) = self.types().int_bits(ty) else { return };
        let piece_bits = 8 * reg_bytes;
        if bits <= piece_bits || bits % piece_bits == 0 {
            return;
        }
        let last = (bits - 1) / piece_bits * reg_bytes;
        let size = bits.div_ceil(8) - last;
        match (part.place, to_stack) {
            (Place::Reg(reg), None) if part.offset == last => t.extend_arg(self, reg, ty, attrs),
            (Place::Stack(_), Some(dst))
                if extends(attrs) && part.offset <= last && last < part.offset + part.size =>
            {
                let scratch = self.scratch_reg(Class::Gpr);
                t.load(self, scratch, slot.offset_by(last), size);
                t.extend_arg(self, scratch, ty, attrs);
                t.store(self, scratch, dst.offset_by(last - part.offset), reg_bytes);
            }
            _ => {}
        }
    }
}
