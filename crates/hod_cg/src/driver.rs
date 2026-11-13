//! The driver: one walk over one function (`spec/codegen.md` §1, §4.7,
//! §4.8, §5.1, §6.1, §6.3, §9).
//!
//! `compile_function` owns the walk and everything that does not depend
//! on the instruction set: which blocks are emitted and in which order,
//! the entry code for the arguments, the body's stack slots, the `phi`
//! protocol around each block, skipping dead instructions, releasing
//! operands, the records, and the retry in far mode.

use hod_ir::{
    Block, BlockId, Body, DbgRecordKind, FnAttrs, Inst, InstKind, LocId, Module, Opcode,
    ParamAttrs, SymbolKind, TypeId, TypeKind, UwTable, Value,
};

use crate::abi::{AbiUnsupported, LaneLoc, PartKind, Place, Sig};
use crate::cx::{FnCx, Scratch};
use crate::error::{
    CALL_BYVAL_PLACE, CALL_PART_CLASS, CgError, CgResult, DEBUG_DBG_ASSIGN, DEBUG_DBG_VALUE,
    DEBUG_DECLARE_ADDRESS, DEBUG_FRAME_PLACE, LIMIT_FUNCTION_SIZE, internal,
};
use crate::frame::{FrameRef, SlotPlace};
use crate::label::Label;
use crate::loc::{Class, Key, Kind, Loc, ValState};
use crate::options::Options;
use crate::record::{BlockOffset, CfiOp, Chunk, FrameDesc, LineRow, VarPlace, VarRow};
use crate::reg::{MIN_POOL_FPR, MIN_POOL_GPR, RegSet};
use crate::target::Target;

/// The most code one function may have, in bytes. No target's far
/// branch reaches further, and code offsets stay well inside 32 bits.
pub const MAX_FUNCTION_SIZE: u32 = 1 << 30;

/// The most code a chunk may hold before another function is compiled
/// into it: offsets into a chunk's tables are 32 bits wide.
const MAX_CHUNK_CODE: usize = 1 << 31;

/// Compiles `body` for `target` and appends the code and its records to
/// `chunk` (§1): one walk, no machine IR.
///
/// `scratch` is the worker's reusable state (§11); `chunk` is the
/// worker's output. On an error the chunk is exactly as it was before
/// the call, so the caller may go on with the next function.
///
/// A function whose short branch forms do not reach is compiled a second
/// time in far mode (§6.3); the first attempt leaves no trace.
///
/// # Errors
/// [`CgError::Unsupported`] for a limit only code generation can know
/// (`limit:frame-size`, `limit:function-size`), for a signature the
/// target's `assign` refuses, and for the `call:` cases of
/// [`crate::error`]; [`CgError::Internal`] for a case `classify` should
/// have rejected, invalid IR, or a failed invariant of the policy.
pub fn compile_function<T: Target>(
    target: &T,
    module: &Module,
    body: &Body,
    options: &Options,
    scratch: &mut Scratch,
    chunk: &mut Chunk,
) -> CgResult<()> {
    if chunk.code.len() >= MAX_CHUNK_CODE {
        return internal("a chunk with more than 2 GiB of code");
    }
    let mark = chunk.mark();
    let own = target.for_function(module, body);
    let target = own.as_ref().unwrap_or(target);
    let mut far = options.stress.far;
    loop {
        let mut cx = FnCx::new(target, module, body, options, scratch, chunk, far);
        let result = cx.run();
        let out_of_range = cx.out_of_range;
        let Err(error) = result else { return Ok(()) };
        chunk.truncate(mark);
        if !out_of_range {
            return Err(error);
        }
        if far {
            return Err(CgError::unsupported(LIMIT_FUNCTION_SIZE));
        }
        far = true;
    }
}

/// Whether an instruction whose result has no use can be left out
/// (§4.7): it has no effect besides its result.
fn has_no_effect(inst: Inst<'_>) -> bool {
    let op = inst.opcode();
    if op.is_binary() || op.is_cast() {
        return true;
    }
    match op {
        Opcode::FNeg
        | Opcode::ICmp
        | Opcode::FCmp
        | Opcode::PtrAdd
        | Opcode::Select
        | Opcode::Freeze
        | Opcode::ExtractValue
        | Opcode::InsertValue
        | Opcode::ExtractElement
        | Opcode::InsertElement
        | Opcode::ShuffleVector => true,
        Opcode::Load => matches!(
            inst.kind(),
            InstKind::Load { mem, .. } if !mem.is_volatile() && mem.ordering().is_none()
        ),
        _ => false,
    }
}

impl<'a, T: Target> FnCx<'a, T> {
    /// A context for one attempt at one function, with the scratch
    /// state reset for it.
    fn new(
        target: &'a T,
        module: &'a Module,
        body: &'a Body,
        options: &'a Options,
        s: &'a mut Scratch,
        out: &'a mut Chunk,
        far: bool,
    ) -> FnCx<'a, T> {
        let blocks = body.block_count();
        let has_pads = body.has_landing_pads();
        s.vals.clear();
        s.vals.resize(body.inst_count(), ValState::UNKNOWN);
        s.args.clear();
        s.args.resize(body.arg_count() as usize, ValState::UNKNOWN);
        s.occupied = RegSet::EMPTY;
        s.pinned = RegSet::EMPTY;
        s.temps = RegSet::EMPTY;
        s.frame.reset(T::STACK_ALIGN);
        s.labels.reset(blocks as u32);
        s.slots.clear();
        s.reachable.clear();
        s.reachable.resize(blocks, false);
        s.work.clear();
        s.phi_mark.clear();
        s.phi_mark.resize(blocks, 0);
        s.pad_of_block.clear();
        if has_pads {
            s.pad_of_block.resize(blocks, 0);
        }
        s.site_pads.clear();
        s.tail_exits.clear();
        s.islands.clear();
        s.island_data.clear();
        s.dbg_labels.clear();
        let epilogue = s.labels.create();
        let ri = *target.reg_info();
        let pool = if options.stress.min_pool {
            let gpr = ri.pool.intersection(ri.gpr).lowest_n(MIN_POOL_GPR);
            gpr.union(ri.pool.intersection(ri.fpr).lowest_n(MIN_POOL_FPR))
        } else {
            ri.pool
        };
        let mark = out.mark();
        FnCx {
            module,
            body,
            target,
            options,
            state: T::FnState::default(),
            s,
            out,
            mark,
            ri,
            pool,
            far,
            out_of_range: false,
            cur: hod_ir::InstId::new(0),
            block: body.entry(),
            next_block: None,
            skip_rest: false,
            no_defer: false,
            deferred_run: 0,
            epilogue,
            body_cfi: u32::MAX,
            has_pads,
            block_addrs: false,
            frame_desc: FrameDesc::default(),
            island_align: 1,
            ret_attrs: ParamAttrs::NONE,
            lines: body.has_locs(),
            last_loc: None,
            line_seg: (0, LocId::NONE),
            line_block_start: false,
            rets: 0,
            ret_loc: LocId::NONE,
            coro: None,
        }
    }

    /// The whole function, once.
    fn run(&mut self) -> CgResult<()> {
        let t = self.target;
        let body = self.body;
        if let Err(what) = self.ri.check() {
            return internal(format!("the target's register description is wrong: {what}"));
        }
        if body.block_count() == 0 || body.block_count() >= 1 << 30 {
            return internal("a body without a block, or with more blocks than labels");
        }
        self.mark_reachable()?;
        self.assign_own()?;
        self.coro_plan()?;
        t.begin_function(self)?;
        self.coro_setup()?;
        self.layout_slots()?;
        self.enter_arguments()?;
        self.enter_slots()?;

        let layout = body.layout();
        let mut at = self.next_reachable(layout, 0);
        if at.map(|index| layout[index]) != Some(body.entry()) {
            // The prologue falls into the first block emitted.
            t.jump(self, self.block_label(body.entry()))?;
        }
        while let Some(index) = at {
            let next = self.next_reachable(layout, index + 1);
            self.next_block = next.map(|index| layout[index]);
            self.emit_block(body.block(layout[index]))?;
            at = next;
        }
        if self.s.labels.offset(self.block_label(body.entry())).is_none() {
            return internal("the entry block is not in the body's layout");
        }

        if self.lines {
            // §9.5: the shared epilogue does not inherit the line of
            // whichever block was emitted last.
            let loc = if self.rets == 1 { self.ret_loc } else { LocId::NONE };
            let offset = self.offset();
            self.out.lines.push(LineRow { offset, loc, block_start: false });
        }
        t.end_function(self)?;
        self.coro_finish()?;
        self.finish_function()
    }

    fn next_reachable(&self, layout: &[BlockId], from: usize) -> Option<usize> {
        (from..layout.len()).find(|&index| self.is_reachable(layout[index]))
    }

    /// §6.1: one walk over terminators from the entry block.
    fn mark_reachable(&mut self) -> CgResult<()> {
        let body = self.body;
        let entry = body.entry();
        let Some(seen) = self.s.reachable.get_mut(entry.index()) else {
            return internal("the entry block is not a block of the body");
        };
        *seen = true;
        self.s.work.push(entry.raw());
        while let Some(raw) = self.s.work.pop() {
            let block = body.block(BlockId::new(raw));
            if block.inst_range().is_empty() {
                return internal(format!("block {raw} is empty"));
            }
            let terminator = block.terminator();
            if terminator.opcode() == Opcode::IndirectBr {
                self.block_addrs = true;
            }
            for succ in terminator.successors() {
                let Some(seen) = self.s.reachable.get_mut(succ.index()) else {
                    return internal(format!(
                        "block {raw} branches to a block that does not exist"
                    ));
                };
                if !*seen {
                    *seen = true;
                    self.s.work.push(succ.raw());
                }
            }
        }
        // WP-095: an `indirectbr` only in unreachable blocks still takes
        // the function's block addresses (`llc` keeps their labels).
        if !self.block_addrs {
            self.block_addrs = self.s.reachable.iter().enumerate().any(|(index, &seen)| {
                let block = body.block(BlockId::new(index as u32));
                !seen
                    && !block.inst_range().is_empty()
                    && block.terminator().opcode() == Opcode::IndirectBr
            });
        }
        Ok(())
    }

    /// §7.2: where the function's own arguments arrive and its return
    /// value leaves.
    fn assign_own(&mut self) -> CgResult<()> {
        let symbol = self.module.symbol(self.body.symbol());
        let SymbolKind::Function { cc, ret_attrs, param_attrs, .. } = symbol.kind() else {
            return internal("a body whose symbol is no function");
        };
        let TypeKind::Function { ret, params, variadic } = self.types().kind(self.body.fn_type())
        else {
            return internal("a body whose type is no function type");
        };
        if params.len() != self.body.arg_count() as usize {
            return internal("a body whose argument count is not its type's");
        }
        let sig =
            Sig { cc, ret, ret_attrs, params, param_attrs, fixed: params.len() as u32, variadic };
        let mut asg = std::mem::take(&mut self.s.fn_asg);
        asg.clear();
        let assigned = self.target.assign(self.types(), &sig, &mut asg);
        self.s.fn_asg = asg;
        self.ret_attrs = ret_attrs;
        assigned.map_err(|AbiUnsupported(name)| CgError::unsupported(name))
    }

    /// §5.1: the body's stack slots, before the first instruction. An
    /// over-aligned one gets a pointer slot here and its memory in
    /// [`FnCx::enter_slots`].
    fn layout_slots(&mut self) -> CgResult<()> {
        let pointer = self.reg_bytes(Class::Gpr);
        for (index, slot) in self.body.slots().iter().enumerate() {
            let place = if let Some(place) = self.coro_slot_place(index)? {
                place
            } else if slot.align() <= u64::from(T::STACK_ALIGN) {
                SlotPlace::Direct(self.s.frame.alloc(slot.size(), slot.align() as u32)?)
            } else {
                SlotPlace::Indirect(
                    self.s.frame.alloc(u64::from(pointer), pointer.min(T::STACK_ALIGN))?,
                )
            };
            self.s.slots.push(place);
        }
        // §9.5: one row per declared variable; `finish_function` adds
        // the declarations of values that are not slots, once their
        // homes are known, and makes every place an offset.
        for var in self.body.slot_vars() {
            let place = match self.slot_place(var.slot)? {
                SlotPlace::Direct(slot) => VarPlace::Slot(slot),
                SlotPlace::Indirect(pointer) => VarPlace::Indirect(pointer),
            };
            self.out.vars.push(VarRow { var: var.var, loc: var.loc, expr: var.expr, place });
        }
        // §9.5: a label is a row at the offset of the instruction it
        // stands before; a `#dbg_value` or `#dbg_assign` is a refused row.
        for record in self.body.dbg_records() {
            let refused = match record.kind {
                DbgRecordKind::Label => {
                    self.s.dbg_labels.push((record.before.raw(), record.var, record.loc));
                    continue;
                }
                DbgRecordKind::Value => DEBUG_DBG_VALUE,
                DbgRecordKind::Assign => DEBUG_DBG_ASSIGN,
            };
            let place = VarPlace::Refused(refused);
            self.out.vars.push(VarRow {
                var: record.var,
                loc: record.loc,
                expr: record.expr,
                place,
            });
        }
        Ok(())
    }

    /// §5.1: the entry code that allocates the over-aligned slots.
    fn enter_slots(&mut self) -> CgResult<()> {
        let t = self.target;
        for (index, slot) in self.body.slots().iter().enumerate() {
            if let Some(&SlotPlace::Indirect(pointer)) = self.s.slots.get(index) {
                t.dynamic_slot(self, pointer, slot.size(), slot.align())?;
            }
        }
        Ok(())
    }

    /// A `Mem`-class argument in registers, or in several pieces:
    /// gathered in its home slot (`parts` of the function's assignment).
    fn enter_mem(
        &mut self,
        ty: TypeId,
        state: ValState,
        parts: std::ops::Range<usize>,
    ) -> CgResult<()> {
        let t = self.target;
        let Loc::Frame(home) = state.loc else {
            return self.bug("a Mem-class argument without a home");
        };
        for at in parts {
            let part = self.s.fn_asg.args[at];
            let to = home.offset_by(part.offset);
            if crate::call::is_lanes(part.kind) {
                t.lanes_in(self, &part, incoming(part.place), LaneLoc::Frame(to))?;
                continue;
            }
            match part.place {
                Place::Reg(reg) => t.store(self, reg, to, part.size),
                Place::Stack(offset) => t.copy(self, to, FrameRef::Incoming(offset), part.size),
            }
        }
        self.clear_odd_leaves(ty, home);
        Ok(())
    }

    /// WP-082: a mask whose elements arrive as lanes (`parts` of the
    /// function's assignment); their bits are gathered in a scratch
    /// register, then moved to the argument's register or home.
    fn enter_lanes(
        &mut self,
        key: Key,
        state: ValState,
        class: Class,
        parts: std::ops::Range<usize>,
    ) -> CgResult<()> {
        let t = self.target;
        let bits = self.scratch_reg(Class::Gpr);
        for at in parts {
            let part = self.s.fn_asg.args[at];
            t.lanes_in(self, &part, incoming(part.place), LaneLoc::Reg(bits))?;
        }
        match (state.kind, state.loc) {
            (Kind::Reg, _) => {
                let to = self.alloc(class)?;
                t.mov(self, to, bits);
                self.own(to, key);
            }
            (Kind::Home, Loc::Frame(home)) => t.store(self, bits, home, u32::from(state.width)),
            _ => return self.bug("an argument without a kind"),
        }
        Ok(())
    }

    /// §4.8: the entry code for the arguments. An argument that arrived
    /// in a register rests there if it is a register value, and is
    /// stored to its home otherwise; one that arrived on the stack has
    /// its incoming location as its home. A `Mem`-class home then has
    /// the bits above its odd-width leaves cleared (§2.2).
    fn enter_arguments(&mut self) -> CgResult<()> {
        let t = self.target;
        let types = self.body.arg_types(self.module);
        let mut next = 0;
        for (index, &ty) in types.iter().enumerate() {
            let key = Key::arg(index as u32);
            let first = next;
            while self.s.fn_asg.args.get(next).is_some_and(|part| part.arg as usize == index) {
                next += 1;
            }
            let uses = self.body.arg_uses(index as u32);
            if (uses.is_unused() && !uses.named_by_declare()) || first == next {
                // Dead, or an argument that takes no space.
                self.ensure(key)?;
                continue;
            }
            let part = self.s.fn_asg.args[first];
            let class = t.class_of(self.types(), ty);

            if part.kind == PartKind::Pointee {
                // `byval`: the value is the address of the copy the
                // caller made in its stack-argument area.
                let Some(base) = (match part.place {
                    Place::Stack(offset) => offset.checked_sub(part.offset),
                    Place::Reg(_) => None,
                }) else {
                    return Err(CgError::unsupported(CALL_BYVAL_PLACE));
                };
                let state = self.ensure(key)?;
                let copy = FrameRef::Incoming(base);
                match (state.kind, state.loc) {
                    (Kind::Reg, _) => {
                        let reg = self.alloc(state.class)?;
                        t.frame_addr(self, reg, copy);
                        self.own(reg, key);
                    }
                    (Kind::Home, Loc::Frame(home)) => {
                        let scratch = self.scratch_reg(Class::Gpr);
                        t.frame_addr(self, scratch, copy);
                        t.store(self, scratch, home, u32::from(state.width));
                    }
                    _ => return self.bug("a byval argument without a kind"),
                }
                continue;
            }

            // Wholly on the stack: the incoming location is the home.
            if next - first == 1
                && part.offset == 0
                && part.kind == PartKind::Value
                && let Place::Stack(offset) = part.place
            {
                let width = if class == Class::Mem { 0 } else { part.size.min(255) as u8 };
                if class == Class::Mem {
                    self.clear_odd_leaves(ty, FrameRef::Incoming(offset));
                }
                let state = ValState {
                    loc: Loc::Frame(FrameRef::Incoming(offset)),
                    kind: Kind::Home,
                    class,
                    width,
                    incoming: crate::loc::NO_SLOT,
                };
                if let Some(entry) = self.state_mut(key) {
                    *entry = state;
                }
                continue;
            }

            let state = self.ensure(key)?;
            if class == Class::Mem {
                self.enter_mem(ty, state, first..next)?;
                continue;
            }
            if crate::call::is_lanes(part.kind) {
                self.enter_lanes(key, state, class, first..next)?;
                continue;
            }
            let (Place::Reg(reg), true) = (part.place, next - first == 1) else {
                return self.bug("a scalar argument in more than one part");
            };
            let of_class = if class == Class::Fpr { self.ri.fpr } else { self.ri.gpr };
            if !of_class.contains(reg) {
                return Err(CgError::unsupported(CALL_PART_CLASS));
            }
            match (state.kind, state.loc) {
                (Kind::Reg, _) if self.ri.holdable.contains(reg) => self.own(reg, key),
                (Kind::Reg, _) => {
                    let to = self.alloc(class)?;
                    t.mov(self, to, reg);
                    self.own(to, key);
                }
                (Kind::Home, Loc::Frame(home)) => t.store(self, reg, home, u32::from(state.width)),
                _ => return self.bug("an argument without a kind"),
            }
        }
        Ok(())
    }

    /// One reachable block: its label, its `phi`s, its instructions.
    fn emit_block(&mut self, block: Block<'a>) -> CgResult<()> {
        let t = self.target;
        let id = block.id();
        self.block = id;
        self.skip_rest = false;
        self.bind(self.block_label(id))?;
        self.coro_block_start(id);
        let mut start = self.offset();
        // §9.5, `spec/dwarf.md` §5.3: whether no instruction of the block
        // has emitted code yet.
        self.line_block_start = id != self.body.entry();
        if self.block_addrs {
            self.out.block_offsets.push(BlockOffset { block: id, offset: start });
        }
        if self.has_pads
            && let Some(first) = block.non_phis().next()
            && let InstKind::LandingPad(pad) = first.kind()
        {
            // §4.8: the `eh_data` registers are not in the pool, so
            // nothing takes them before the `LandingPad` reads them.
            self.begin_pad(id, pad.is_cleanup());
        }
        self.phi_loads(block)?;

        for inst in block.non_phis() {
            self.cur = inst.id();
            self.line_begin(crate::line_rows::row_loc(inst));
            if !self.s.dbg_labels.is_empty() {
                self.label_rows(inst.id().raw());
            }
            let dead = inst.ty() != TypeId::VOID && inst.uses().is_unused() && has_no_effect(inst);
            if !dead && self.coro.is_some() && self.coro_switch(inst)? {
                // WP-106: a suspend switch writes its edges itself.
            } else if !dead {
                if inst.is_terminator() && inst.opcode() != Opcode::Invoke {
                    // §4.5: for all successors, whichever is taken. An
                    // `invoke`'s are written by the call sequence.
                    for succ in inst.successors() {
                        self.phi_stores(succ)?;
                    }
                }
                t.lower(self, inst)?;
                if inst.opcode() == Opcode::Ret {
                    self.rets += 1;
                    self.ret_loc = inst.loc();
                }
            }
            // §4.2: the operands are released here, not by lowerings;
            // a deferred instruction's stay held for the next one.
            let deferred = !dead
                && inst.ty() != TypeId::VOID
                && self.s.vals.get(inst.id().index()).is_some_and(|s| s.loc == Loc::Deferred);
            if !deferred {
                for &operand in inst.operands() {
                    self.release(operand);
                }
                self.deferred_run = 0;
            }
            self.s.pinned = RegSet::EMPTY;
            self.s.temps = RegSet::EMPTY;
            self.s.frame.end_instruction();
            if self.options.stress.displace {
                self.displace_all()?;
            }

            // §9.5: transitions only, and an instruction without a
            // location that starts a block (`spec/dwarf.md` §5.3, §5.8).
            self.line_end();
            let end = self.offset();
            if end != start {
                if end > MAX_FUNCTION_SIZE {
                    return Err(CgError::unsupported(LIMIT_FUNCTION_SIZE));
                }
                start = end;
            }
            if self.skip_rest {
                // §7.1: after a `musttail` call the rest of the block is
                // not emitted, and what it would have consumed is gone.
                self.s.occupied = RegSet::EMPTY;
                return Ok(());
            }
        }
        if !self.s.occupied.is_empty() {
            // §4.5: no register holds a value at the end of a block.
            return internal(format!(
                "block {} ends with a value in a register ({:?})",
                id.raw(),
                self.s.occupied
            ));
        }
        Ok(())
    }

    /// States the body's call-frame rules again at the current offset
    /// (§9.3), for code after the exit sequences that the body branches
    /// to: the net effect of the rows emitted before the epilogue was
    /// bound, as one CFA rule and one rule per register saved.
    fn restate_body_cfi(&mut self) {
        if self.body_cfi == u32::MAX {
            return;
        }
        let (start, end) = (self.mark.cfi as usize, (self.mark.cfi + self.body_cfi) as usize);
        let (mut reg, mut offset) = (None, None);
        for row in &self.out.cfi[start..end] {
            match row.op {
                CfiOp::DefCfa { reg: r, offset: o } => (reg, offset) = (Some(r), Some(o)),
                CfiOp::DefCfaOffset(o) => offset = Some(o),
                CfiOp::DefCfaRegister(r) => reg = Some(r),
                CfiOp::Offset { .. } | CfiOp::Restore { .. } => {}
            }
        }
        match (reg, offset) {
            (Some(reg), Some(offset)) => self.cfi(CfiOp::DefCfa { reg, offset }),
            (None, Some(offset)) => self.cfi(CfiOp::DefCfaOffset(offset)),
            (Some(reg), None) => self.cfi(CfiOp::DefCfaRegister(reg)),
            (None, None) => {}
        }
        for index in start..end {
            let CfiOp::Offset { reg, .. } = self.out.cfi[index].op else { continue };
            // A register's rule is its last row's.
            let later = self.out.cfi[index + 1..end].iter().any(|row| {
                matches!(row.op, CfiOp::Offset { reg: r, .. } | CfiOp::Restore { reg: r } if r == reg)
            });
            if !later {
                let op = self.out.cfi[index].op;
                self.cfi(op);
            }
        }
    }

    /// §9.5: the rows of the `#dbg_label`s that stand before instruction
    /// `raw`, at the offset where its code starts.
    fn label_rows(&mut self, raw: u32) {
        let labels = &self.s.dbg_labels;
        let mut at = labels.partition_point(|&(before, _, _)| before < raw);
        let offset = self.offset();
        while let Some(&(before, var, loc)) = self.s.dbg_labels.get(at) {
            if before != raw {
                break;
            }
            let row =
                VarRow { var, loc, expr: hod_ir::ExprId::EMPTY, place: VarPlace::Label(offset) };
            self.out.vars.push(row);
            at += 1;
        }
    }

    /// §9.5, once the frame is final: the rows of the declarations of
    /// values that are not slots, and every frame place made an offset
    /// from the frame base.
    fn finish_vars(&mut self) {
        let body = self.body;
        for declare in body.declares() {
            if declare.address == Value::NONE {
                // The address was dropped: no row, as `llc` writes none.
                continue;
            }
            // §4.1: a declared value has a home for the whole function.
            let place = match Key::of(declare.address).map(|key| self.state(key)) {
                Some(Ok(ValState { kind: Kind::Home, loc: Loc::Frame(home), .. })) => {
                    VarPlace::Indirect(home)
                }
                _ => VarPlace::Refused(DEBUG_DECLARE_ADDRESS),
            };
            let row = VarRow { var: declare.var, loc: declare.loc, expr: declare.expr, place };
            self.out.vars.push(row);
        }
        let t = self.target;
        let desc = self.frame_desc;
        let start = self.mark.vars as usize;
        for row in self.out.vars.get_mut(start..).unwrap_or_default() {
            let (place, indirect) = match row.place {
                VarPlace::Slot(place) => (place, false),
                VarPlace::Indirect(place) => (place, true),
                _ => continue,
            };
            row.place = match t.frame_base_offset(&desc, place) {
                Some(offset) if indirect => VarPlace::FrameIndirect(offset),
                Some(offset) => VarPlace::Frame(offset),
                None => VarPlace::Refused(DEBUG_FRAME_PLACE),
            };
        }
    }

    /// What follows the target's exit sequences: the trap that stands
    /// for unreachable blocks, the data islands, the last checks, the
    /// function's record.
    fn finish_function(&mut self) -> CgResult<()> {
        // §6.1: a label that must exist for an unreachable block is
        // bound to a trap at the end of the function.
        if self.block_addrs || self.s.labels.pending() != 0 {
            let mut trap = None;
            for raw in 0..self.body.block_count() as u32 {
                let label = Label(raw);
                if self.s.reachable.get(raw as usize) != Some(&false)
                    || !(self.block_addrs || self.s.labels.has_pending(label))
                {
                    continue;
                }
                if trap.is_none() {
                    trap = Some(self.offset());
                    // §9.3: the trap follows the exit sequences, and the
                    // body branches to it.
                    self.restate_body_cfi();
                    self.trap();
                }
                // Every unreachable block shares the one trap: binding
                // a label there, not at the current offset.
                let at = trap.unwrap_or(0);
                self.bind_at(label, at)?;
                if self.block_addrs {
                    self.out
                        .block_offsets
                        .push(BlockOffset { block: BlockId::new(raw), offset: at });
                }
            }
        }
        self.emit_islands()?;
        if self.s.labels.pending() != 0 {
            return internal("a label that code refers to was never bound");
        }
        if self.offset() > MAX_FUNCTION_SIZE {
            return Err(CgError::unsupported(LIMIT_FUNCTION_SIZE));
        }

        // §9.4: an `invoke`'s row names its pad once the pad exists.
        for index in 0..self.s.site_pads.len() {
            let block = self.s.site_pads[index];
            if block == u32::MAX {
                continue;
            }
            let pad = self.s.pad_of_block.get(block as usize).copied().unwrap_or(0);
            let row = self.mark.call_sites as usize + index;
            let offset = self.out.pads.get((self.mark.pads + pad).wrapping_sub(1) as usize);
            let (Some(offset), true) = (offset.map(|pad| pad.offset), pad != 0) else {
                return internal("an invoke unwinds to a block that is no landing pad");
            };
            let Some(row) = self.out.call_sites.get_mut(row) else {
                return internal("a call site without its row");
            };
            row.pad = pad - 1;
            if row.landing == 0 {
                row.landing = offset;
            }
        }

        self.finish_vars();

        let mut record = self.out.record(self.mark, self.body.symbol());
        record.align = T::FN_ALIGN.max(self.island_align);
        record.frame = self.frame_desc;
        record.has_pads = self.has_pads;
        record.coro_entries = self.coro_entries();
        // `spec/eh.md` §1.1: what decides the function's unwind tables.
        if let SymbolKind::Function { attrs, personality, .. } =
            self.module.symbol(self.body.symbol()).kind()
        {
            let attrs = self.module.fn_attrs(attrs);
            record.personality = personality;
            record.nounwind = attrs.has(FnAttrs::NOUNWIND);
            record.uwtable = attrs.uwtable() != UwTable::None;
        }
        self.out.funcs.push(record);
        Ok(())
    }
}

/// A `Lanes` part's incoming place, as a side of `Target::lanes_in`.
fn incoming(place: Place) -> LaneLoc {
    match place {
        Place::Reg(reg) => LaneLoc::Reg(reg),
        Place::Stack(offset) => LaneLoc::Frame(FrameRef::Incoming(offset)),
    }
}
