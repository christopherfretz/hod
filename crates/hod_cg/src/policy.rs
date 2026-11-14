//! The value-location policy v1: `spec/codegen.md` §4.1 to §4.4, §4.6.
//!
//! Every tracked value (an instruction result or an argument) is, from
//! its use facts and its class, one of three kinds, decided at its first
//! mention:
//!
//! - **dead**: no use; it lives nowhere;
//! - a **register value**: one use, in the defining block; it lives in a
//!   register from its definition to its use, or in the spill slot it
//!   was displaced to;
//! - a **home value**: anything else, and every used `Mem`-class value;
//!   it lives in a frame slot that is written at the definition and read
//!   at each use.
//!
//! So a register holds a value only between a definition and the one use
//! that follows it in the same block, and nothing has to be reconciled
//! at a block boundary.

use hod_ir::{Inst, InstId, InstKind, TypeId, TypeKind, Value, ValueKind};

use crate::cx::FnCx;
use crate::error::{CgError, CgResult, internal};
use crate::frame::FrameRef;
use crate::loc::{Class, Key, Kind, Loc, ValState};
use crate::reg::{PReg, RegSet};
use crate::target::Target;

/// The longest run of consecutive deferred instructions (§4.6). Asking
/// for a deferred value emits it through its lowering, which may ask for
/// a deferred operand in turn: the bound keeps that recursion shallow
/// whatever the input.
pub const MAX_DEFERRED_RUN: u32 = 8;

impl<'a, T: Target> FnCx<'a, T> {
    // ---- the location table ----

    /// The table entry of `key`.
    #[inline]
    pub(crate) fn state(&self, key: Key) -> CgResult<ValState> {
        let found = match key.as_arg() {
            Some(index) => self.s.args.get(index as usize),
            None => self.s.vals.get(key.0 as usize),
        };
        match found {
            Some(&state) => Ok(state),
            None => internal(format!("value {:#x} is not a value of this body", key.0)),
        }
    }

    #[inline]
    pub(crate) fn state_mut(&mut self, key: Key) -> Option<&mut ValState> {
        match key.as_arg() {
            Some(index) => self.s.args.get_mut(index as usize),
            None => self.s.vals.get_mut(key.0 as usize),
        }
    }

    #[inline]
    pub(crate) fn set_loc(&mut self, key: Key, loc: Loc) {
        if let Some(state) = self.state_mut(key) {
            state.loc = loc;
        }
    }

    /// The table entry of `key`, decided now if this is the value's
    /// first mention (§4.1).
    #[inline]
    pub(crate) fn ensure(&mut self, key: Key) -> CgResult<ValState> {
        let state = self.state(key)?;
        if state.kind != Kind::Unknown {
            return Ok(state);
        }
        self.classify(key)
    }

    /// §4.1: the kind of a value from its use facts and its class, and
    /// its home slot if it is a home value.
    #[cold]
    #[inline(never)]
    fn classify(&mut self, key: Key) -> CgResult<ValState> {
        let (ty, uses) = if let Some(index) = key.as_arg() {
            let Some(&ty) = self.body.arg_types(self.module).get(index as usize) else {
                return internal(format!("argument {index} is not an argument of this body"));
            };
            (ty, self.body.arg_uses(index))
        } else {
            let id = InstId::new(key.0);
            (self.body.inst(id).ty(), self.body.uses(id))
        };
        if ty == TypeId::VOID {
            return internal(format!("value {:#x} has no type: it is not a value", key.0));
        }
        let class = self.target.class_of(self.types(), ty);
        if uses.named_by_declare() && key.as_arg().is_none() {
            self.check_declared(InstId::new(key.0))?;
        }
        let kind = if uses.named_by_declare() {
            // §4.1: a variable's address for the whole function.
            Kind::Home
        } else if uses.is_unused() {
            Kind::Dead
        } else if class == Class::Mem || uses.count() > 1 || uses.used_outside_block() {
            Kind::Home
        } else {
            Kind::Reg
        };
        let (width, loc) = if kind == Kind::Home {
            let block = key.as_arg().is_none()
                && !uses.used_outside_block()
                && !uses.named_by_declare()
                && self.block_slots
                && self.body.inst(InstId::new(key.0)).opcode() != hod_ir::Opcode::Phi
                && self.body.block(self.block).inst_range().contains(&key.0);
            let slot = match self.takes_over(key, class, block)? {
                Some(slot) => slot,
                None => self.home_slot(class, ty, block)?,
            };
            (self.home_bytes(class, ty) as u8, Loc::Frame(slot))
        } else {
            (self.reg_bytes(class) as u8, Loc::None)
        };
        let state = ValState { loc, kind, class, width, incoming: crate::loc::NO_SLOT };
        if let Some(entry) = self.state_mut(key) {
            *entry = state;
        }
        Ok(state)
    }

    /// The block slot an `insertvalue` result of class `Mem` that gets a
    /// block slot itself takes over from its aggregate operand (§5.1
    /// "Value slots"): the operand's, when the operand is an instruction
    /// result of this block with a block slot and this is its only use.
    /// The lowering copies the aggregate into the result's slot (nothing
    /// when they are one) and then writes the field, and nothing reads
    /// the operand afterwards.
    fn takes_over(&self, key: Key, class: Class, block: bool) -> CgResult<Option<FrameRef>> {
        if !block || class != Class::Mem {
            return Ok(None);
        }
        let InstKind::InsertValue { agg, .. } = self.body.inst(InstId::new(key.0)).kind() else {
            return Ok(None);
        };
        let Some(id) = agg.as_inst() else { return Ok(None) };
        let uses = self.body.uses(id);
        if uses.count() != 1
            || uses.used_outside_block()
            || uses.named_by_declare()
            || self.body.inst(id).opcode() == hod_ir::Opcode::Phi
            || !self.body.block(self.block).inst_range().contains(&id.raw())
        {
            return Ok(None);
        }
        match self.state(Key::inst(id.raw()))? {
            ValState { kind: Kind::Home, class: Class::Mem, loc: Loc::Frame(slot), .. } => {
                Ok(Some(slot))
            }
            _ => Ok(None),
        }
    }

    /// A `#dbg_declare` of a stack slot plus a constant offset (a
    /// `getelementptr` of an `alloca`, a slot, with constant indices) is refused
    /// by name (WP-150, `spec/dwarf.md` §6.21): the target folds such an
    /// address into its users and never gives it the home the
    /// declaration needs. `llc` describes it as the slot plus the offset.
    fn check_declared(&self, id: InstId) -> CgResult<()> {
        let inst = self.body.inst(id);
        if let InstKind::PtrAdd(add) = inst.kind()
            && add.index_count() == 0
            && matches!(add.base.kind(), ValueKind::Slot(_))
        {
            return Err(CgError::unsupported(crate::emit::unsupported::DEBUG_DECLARE_OFFSET));
        }
        Ok(())
    }

    /// The bytes of a home or incoming slot of a value of `class` and
    /// type `ty` (§5.1 "Value slots"): an integer's store size rounded
    /// up to a power of two, 4 for a `float`, 8 for a `double`, a
    /// register's bytes for anything else of class `Gpr` or `Fpr`, and
    /// always a register's bytes in a coroutine body (§7.6); zero for
    /// `Mem`. A home is written and read with exactly these bytes.
    #[inline]
    pub(crate) fn home_bytes(&self, class: Class, ty: TypeId) -> u32 {
        let full = self.reg_bytes(class);
        if self.coro.is_some() {
            return full;
        }
        let bytes = match (class, self.types().kind(ty)) {
            (Class::Gpr, TypeKind::Int(bits)) => bits.div_ceil(8).next_power_of_two(),
            (Class::Fpr, TypeKind::Float) => 4,
            (Class::Fpr, TypeKind::Double) => 8,
            _ => full,
        };
        bytes.min(full)
    }

    /// A new home or incoming slot for a value of `class` and type `ty`
    /// (§5.1 "Value slots"): [`FnCx::home_bytes`] wide and aligned, in a
    /// hole if one fits; a block slot if `block`.
    pub(crate) fn home_slot(&mut self, class: Class, ty: TypeId, block: bool) -> CgResult<FrameRef> {
        let (size, align) = match class {
            Class::Gpr | Class::Fpr => {
                let bytes = self.home_bytes(class, ty);
                (u64::from(bytes), bytes.min(T::STACK_ALIGN))
            }
            Class::Mem => {
                let types = self.types();
                let align = types.align_of(ty).min(u64::from(T::STACK_ALIGN)).max(1);
                (types.alloc_size_of(ty), align as u32)
            }
        };
        if block {
            self.s.frame.alloc_block(size, align)
        } else {
            self.s.frame.alloc_value(size, align)
        }
    }

    /// A new slot laid out for a value of `class` and type `ty` (§2.2),
    /// a register's bytes wide for `Gpr` and `Fpr`: a coroutine's shadow
    /// home (§7.6).
    pub(crate) fn value_slot(&mut self, class: Class, ty: TypeId) -> CgResult<FrameRef> {
        match class {
            Class::Gpr | Class::Fpr => {
                let bytes = self.reg_bytes(class);
                self.s.frame.alloc(u64::from(bytes), bytes.min(T::STACK_ALIGN))
            }
            Class::Mem => {
                let types = self.types();
                let align = types.align_of(ty).min(u64::from(T::STACK_ALIGN)).max(1);
                self.s.frame.alloc(types.alloc_size_of(ty), align as u32)
            }
        }
    }

    /// The bytes of a home or spill slot of `class`; zero for `Mem`.
    #[inline]
    pub(crate) fn reg_bytes(&self, class: Class) -> u32 {
        match class {
            Class::Gpr => u32::from(self.ri.gpr_bytes),
            Class::Fpr => u32::from(self.ri.fpr_bytes),
            Class::Mem => 0,
        }
    }

    #[inline]
    fn class_regs(&self, class: Class) -> RegSet {
        match class {
            Class::Gpr => self.ri.gpr,
            Class::Fpr => self.ri.fpr,
            Class::Mem => RegSet::EMPTY,
        }
    }

    /// Makes `reg` the location of `key`.
    #[inline]
    pub(crate) fn own(&mut self, reg: PReg, key: Key) {
        self.s.used = self.s.used.with(reg);
        self.s.occupied = self.s.occupied.with(reg);
        self.s.owner[usize::from(reg.index())] = key;
        self.set_loc(key, Loc::Reg(reg));
    }

    // ---- operands (§4.2) ----

    /// The class of `value`, from its type (§2.2). `value` is a
    /// first-class value: not [`Value::NONE`].
    #[inline]
    pub fn class_of(&self, value: Value) -> Class {
        self.target.class_of(self.types(), self.body.value_type(self.module, value))
    }

    /// Where `value` is (§3). [`Loc::None`] for a value that has no
    /// location: a constant, a symbol's or a stack slot's address, a
    /// dead value, a register value that was consumed.
    ///
    /// # Errors
    /// `Internal` for a value the body does not have.
    #[inline]
    pub fn loc(&mut self, value: Value) -> CgResult<Loc> {
        match Key::of(value) {
            Some(key) => Ok(self.ensure(key)?.loc),
            None => Ok(Loc::None),
        }
    }

    /// `value` in a register of its class, pinned until the instruction
    /// has been emitted (§4.2):
    ///
    /// - a register value is in its register, or is loaded back from
    ///   the spill slot it was displaced to;
    /// - a home value is loaded into a temporary;
    /// - a constant, a symbol's address or a stack slot's address is
    ///   materialized into a temporary (`Target::materialize`);
    /// - a deferred value (§4.6) is emitted now, by its own lowering.
    ///
    /// **The lowering may overwrite the register**, and may leave its
    /// result there ([`FnCx::finish`]): a register value dies at this,
    /// its only use, and anything else was loaded into a temporary.
    /// What it must not do is ask for the same register value twice.
    ///
    /// # Errors
    /// `Internal` for a `Mem`-class value, for a value with no location
    /// (a use before the definition: invalid IR), and when more
    /// registers of the class are pinned than the pool has (§2.1).
    pub fn use_reg(&mut self, value: Value) -> CgResult<PReg> {
        let t = self.target;
        let Some(key) = Key::of(value) else {
            return match value.kind() {
                ValueKind::Const(_) | ValueKind::Sym(_) | ValueKind::Slot(_) => {
                    let class = self.class_of(value);
                    let reg = self.temp(class)?;
                    t.materialize(self, reg, value)?;
                    Ok(reg)
                }
                _ => self.bug("an operand that is no value"),
            };
        };
        let mut state = self.ensure(key)?;
        if state.loc == Loc::Deferred {
            self.force(InstId::new(key.0))?;
            state = self.state(key)?;
        }
        match state.loc {
            Loc::Reg(reg) => {
                self.s.pinned = self.s.pinned.with(reg);
                Ok(reg)
            }
            Loc::Frame(slot) if state.kind == Kind::Home => {
                if state.class == Class::Mem {
                    return self.bug("use_reg of a value of class Mem");
                }
                let reg = self.temp(state.class)?;
                t.load(self, reg, slot, u32::from(state.width));
                Ok(reg)
            }
            Loc::Frame(slot) => {
                // A register value that was displaced: it is a register
                // value again, and its spill slot is free.
                let reg = self.alloc(state.class)?;
                t.load(self, reg, slot, u32::from(state.width));
                self.s.frame.free_spill(state.class, slot);
                self.own(reg, key);
                self.s.pinned = self.s.pinned.with(reg);
                Ok(reg)
            }
            Loc::None | Loc::Deferred => self.bug("a use of a value that has no location"),
        }
    }

    /// Puts `value` in `reg`, a register of its class that the caller
    /// chose: an argument or return register, a scratch register, a
    /// temporary. Nothing is pinned or allocated, and `value` stays
    /// where it was.
    ///
    /// # Errors
    /// As [`FnCx::use_reg`].
    pub fn put(&mut self, value: Value, reg: PReg) -> CgResult<()> {
        let t = self.target;
        let Some(key) = Key::of(value) else {
            return match value.kind() {
                ValueKind::Const(_) | ValueKind::Sym(_) | ValueKind::Slot(_) => {
                    t.materialize(self, reg, value)
                }
                _ => self.bug("an operand that is no value"),
            };
        };
        let mut state = self.ensure(key)?;
        if state.loc == Loc::Deferred {
            self.force(InstId::new(key.0))?;
            state = self.state(key)?;
        }
        match state.loc {
            Loc::Reg(from) => {
                if from != reg {
                    t.mov(self, reg, from);
                }
                Ok(())
            }
            Loc::Frame(slot) if state.class != Class::Mem => {
                t.load(self, reg, slot, u32::from(state.width));
                Ok(())
            }
            _ => self.bug("put of a value that has no location or is of class Mem"),
        }
    }

    /// The frame slot that holds `value`: the home of a `Mem`-class
    /// value (or of any home value, for an instruction that takes a
    /// memory operand), or a slot that lasts for this instruction into
    /// which a `Mem`-class constant is written
    /// (`Target::materialize_mem`).
    ///
    /// # Errors
    /// `Internal` for a value that is in a register or has no location.
    pub fn use_mem(&mut self, value: Value) -> CgResult<FrameRef> {
        if let Some(key) = Key::of(value) {
            return match self.ensure(key)?.loc {
                Loc::Frame(slot) => Ok(slot),
                _ => self.bug("use_mem of a value that is not in a frame slot"),
            };
        }
        if !matches!(value.kind(), ValueKind::Const(_)) || self.class_of(value) != Class::Mem {
            return self.bug("use_mem of an operand that is no constant of class Mem");
        }
        let size = self.types().alloc_size_of(self.body.value_type(self.module, value));
        let slot = self.s.frame.alloc_temp(size)?;
        let t = self.target;
        t.materialize_mem(self, slot, value)?;
        Ok(slot)
    }

    /// The instruction that defines `value`, if it was deferred (§4.6)
    /// and has not been emitted: the lowering may fuse it (use its
    /// operands itself, with [`FnCx::use_reg`] and the rest) or ask for
    /// the value, which emits it.
    #[inline]
    pub fn deferred(&self, value: Value) -> Option<Inst<'a>> {
        let id = value.as_inst()?;
        match self.s.vals.get(id.index()) {
            Some(state) if state.loc == Loc::Deferred => Some(self.body.inst(id)),
            _ => None,
        }
    }

    /// Emits deferred instruction `id` now, by its own lowering, and
    /// leaves its result pinned in a register.
    fn force(&mut self, id: InstId) -> CgResult<()> {
        let key = Key::inst(id.raw());
        let inst = self.body.inst(id);
        self.set_loc(key, Loc::None);
        let (cur, no_defer) = (self.cur, self.no_defer);
        let (pinned, temps) = (self.s.pinned, self.s.temps);
        self.cur = id;
        self.no_defer = true;
        let t = self.target;
        // `spec/dwarf.md` §5.8: its code is at its own location.
        let outer = self.line_loc();
        self.line_switch(crate::line_rows::row_loc(inst));
        let lowered = t.lower(self, inst);
        self.line_switch(outer);
        self.cur = cur;
        self.no_defer = no_defer;
        lowered?;
        for &operand in inst.operands() {
            self.release(operand);
        }
        let Loc::Reg(reg) = self.state(key)?.loc else {
            return self.bug("a deferred instruction left its result nowhere when asked for it");
        };
        // What the deferred instruction pinned and took is free again;
        // what the instruction that asked had pinned stays pinned.
        self.s.temps = temps.without(reg);
        self.s.pinned = pinned.with(reg);
        Ok(())
    }

    /// Releases `value` as an operand that has been consumed (§4.2): a
    /// register value's register or spill slot is free, and a deferred
    /// value's operands are released in turn. Nothing happens for
    /// anything else.
    ///
    /// The driver calls this for every operand once an instruction has
    /// been emitted or skipped; a lowering calls it only to give a
    /// register back early, while it consumes a long operand list one
    /// operand at a time.
    pub fn release(&mut self, value: Value) {
        let Some(key) = Key::of(value) else { return };
        let Ok(state) = self.state(key) else { return };
        if state.kind != Kind::Reg {
            return;
        }
        match state.loc {
            Loc::Reg(reg) => {
                self.s.occupied = self.s.occupied.without(reg);
                self.s.pinned = self.s.pinned.without(reg);
            }
            Loc::Frame(slot) => self.s.frame.free_spill(state.class, slot),
            Loc::Deferred => {
                // At most `MAX_DEFERRED_RUN` deep.
                for &operand in self.body.inst(InstId::new(key.0)).operands() {
                    self.release(operand);
                }
            }
            Loc::None => return,
        }
        self.set_loc(key, Loc::None);
    }

    // ---- results (§4.1) ----

    /// A register for the result of `inst`: a temporary of the result's
    /// class. The lowering computes into it and calls [`FnCx::finish`].
    /// (It may instead compute into a register [`FnCx::use_reg`] gave
    /// it.)
    ///
    /// # Errors
    /// `Internal` for a `Mem`-class result and when the pool is pinned.
    pub fn def_reg(&mut self, inst: Inst<'a>) -> CgResult<PReg> {
        let class = self.ensure(Key::inst(inst.id().raw()))?.class;
        self.temp(class)
    }

    /// The result of `inst` is in `reg`: applies §4.1. A dead result is
    /// dropped, a home value is stored to its home, and a register
    /// value stays in `reg`, which must then be a register a value may
    /// rest in ([`RegInfo::holdable`](crate::RegInfo::holdable)): a pool
    /// register, an operand's register, a return register.
    ///
    /// # Errors
    /// `Internal` for a register value left in a scratch register, and
    /// for a register that holds a value which is not an operand of
    /// `inst`.
    pub fn finish(&mut self, inst: Inst<'a>, reg: PReg) -> CgResult<()> {
        let key = Key::inst(inst.id().raw());
        let state = self.ensure(key)?;
        match state.kind {
            Kind::Dead => Ok(()),
            Kind::Home => {
                let Loc::Frame(home) = state.loc else {
                    return self.bug("a home value without a home");
                };
                if state.class == Class::Mem {
                    return self.bug("finish of a result of class Mem");
                }
                let t = self.target;
                t.store(self, reg, home, u32::from(state.width));
                Ok(())
            }
            Kind::Reg => {
                if !self.ri.holdable.contains(reg) {
                    return self.bug("a result left in a register that holds no value");
                }
                if self.s.occupied.contains(reg) {
                    // The result takes over an operand's register.
                    let previous = self.s.owner[usize::from(reg.index())];
                    if previous != key {
                        if !self.consumes(inst, previous, 0) {
                            return self.bug("a result overwrote a value that is still needed");
                        }
                        self.set_loc(previous, Loc::None);
                    }
                }
                self.s.temps = self.s.temps.without(reg);
                self.s.pinned = self.s.pinned.with(reg);
                self.own(reg, key);
                Ok(())
            }
            Kind::Unknown => self.bug("a value without a kind"),
        }
    }

    /// Whether `key` is an operand of `inst`, or of a deferred
    /// instruction `inst` consumes.
    fn consumes(&self, inst: Inst<'a>, key: Key, depth: u32) -> bool {
        inst.operands().iter().any(|&operand| {
            Key::of(operand) == Some(key)
                || (depth < MAX_DEFERRED_RUN
                    && self.deferred(operand).is_some_and(|d| self.consumes(d, key, depth + 1)))
        })
    }

    /// The frame slot the `Mem`-class result of `inst` is written to:
    /// its home, or a slot that lasts for this instruction when the
    /// result has no use. Nothing is to be finished afterwards: the
    /// slot is the value.
    ///
    /// # Errors
    /// `Internal` for a result that is not of class `Mem`.
    pub fn def_mem(&mut self, inst: Inst<'a>) -> CgResult<FrameRef> {
        let state = self.ensure(Key::inst(inst.id().raw()))?;
        if state.class != Class::Mem {
            return self.bug("def_mem of a result that is not of class Mem");
        }
        if let Loc::Frame(home) = state.loc {
            Ok(home)
        } else {
            let size = self.types().alloc_size_of(inst.ty());
            self.s.frame.alloc_temp(size)
        }
    }

    /// Whether the result of `inst` has no use (§4.7). The driver has
    /// skipped the instruction already if it also has no effect.
    #[inline]
    pub fn is_dead(&self, inst: Inst<'a>) -> bool {
        inst.uses().is_unused()
    }

    // ---- fusing (§4.6) ----

    /// Whether `inst` may be left unemitted for the next instruction to
    /// fuse or ask for: its result's only use is by the instruction
    /// right after it, it is of class `Gpr` or `Fpr`, it is not being
    /// asked for right now, and the run of deferred instructions is
    /// below [`MAX_DEFERRED_RUN`]. Only an instruction without an effect
    /// may be deferred; that is the lowering's to know.
    #[inline]
    pub fn may_defer(&self, inst: Inst<'a>) -> bool {
        !self.no_defer
            && self.deferred_run < MAX_DEFERRED_RUN
            && inst.uses().only_use_is_next()
            && inst.ty() != TypeId::VOID
            && self.target.class_of(self.types(), inst.ty()) != Class::Mem
    }

    /// Leaves `inst` unemitted (§4.6). Its operands stay where they
    /// are, held, until the next instruction has been emitted.
    ///
    /// # Errors
    /// `Internal` unless [`FnCx::may_defer`].
    pub fn defer(&mut self, inst: Inst<'a>) -> CgResult<()> {
        if !self.may_defer(inst) {
            return self.bug("defer of an instruction that may not be deferred");
        }
        let key = Key::inst(inst.id().raw());
        self.ensure(key)?;
        self.set_loc(key, Loc::Deferred);
        self.deferred_run += 1;
        Ok(())
    }

    // ---- registers (§4.3) ----

    /// A pool register of `class` that holds nothing: a free one, or
    /// the lowest-numbered unpinned occupied one, displaced (§4.3).
    pub(crate) fn alloc(&mut self, class: Class) -> CgResult<PReg> {
        let of_class = self.pool.intersection(self.class_regs(class));
        let busy = self.s.occupied.union(self.s.temps);
        if let Some(reg) = of_class.difference(busy).lowest() {
            self.s.used = self.s.used.with(reg);
            return Ok(reg);
        }
        let Some(victim) =
            of_class.intersection(self.s.occupied).difference(self.s.pinned).lowest()
        else {
            return self.bug("no register is free: more are pinned than the pool has");
        };
        self.displace(victim)?;
        Ok(victim)
    }

    /// A temporary register of `class` (`Gpr` or `Fpr`) from the pool,
    /// pinned for the duration of the instruction; free again when the
    /// instruction has been emitted, or at [`FnCx::free_temp`]. A call
    /// the lowering makes ends every temporary.
    ///
    /// # Errors
    /// `Internal` when more registers are pinned than the pool has.
    pub fn temp(&mut self, class: Class) -> CgResult<PReg> {
        let reg = self.alloc(class)?;
        self.s.temps = self.s.temps.with(reg);
        self.s.pinned = self.s.pinned.with(reg);
        Ok(reg)
    }

    /// Gives a temporary back before the instruction ends. Nothing
    /// happens if `reg` is not a temporary (it is a value's register:
    /// see [`FnCx::release`]), so a lowering that consumes a long
    /// operand list calls both for each operand.
    #[inline]
    pub fn free_temp(&mut self, reg: PReg) {
        if self.s.temps.contains(reg) {
            self.s.temps = self.s.temps.without(reg);
            self.s.pinned = self.s.pinned.without(reg);
        }
    }

    /// Makes one particular register a temporary (a shift-count
    /// register, a register an instruction writes implicitly): the
    /// value it holds, if any, is displaced.
    ///
    /// # Errors
    /// `Internal` if `reg` is pinned.
    pub fn take_reg(&mut self, reg: PReg) -> CgResult<()> {
        if self.s.pinned.contains(reg) {
            return self.bug("take_reg of a pinned register");
        }
        self.displace(reg)?;
        self.s.used = self.s.used.with(reg);
        self.s.temps = self.s.temps.with(reg);
        self.s.pinned = self.s.pinned.with(reg);
        Ok(())
    }

    /// Every register the policy has handed out ([`FnCx::temp`],
    /// [`FnCx::take_reg`], a result's or operand's register) or made a
    /// value's location since the function began: with what the target
    /// writes on its own, the registers its prologue must save (§5.6.15).
    #[inline]
    pub fn regs_used(&self) -> RegSet {
        self.s.used
    }

    /// Stores the value `reg` holds, if any, to a spill slot (§4.3).
    ///
    /// # Errors
    /// `Internal` if `reg` is pinned.
    pub fn displace(&mut self, reg: PReg) -> CgResult<()> {
        if !self.s.occupied.contains(reg) {
            return Ok(());
        }
        if self.s.pinned.contains(reg) {
            return self.bug("displacing a pinned register");
        }
        let key = self.s.owner[usize::from(reg.index())];
        let state = self.state(key)?;
        let bytes = u32::from(state.width);
        let slot = self.s.frame.alloc_spill(state.class, bytes)?;
        let t = self.target;
        t.store(self, reg, slot, bytes);
        self.s.occupied = self.s.occupied.without(reg);
        self.set_loc(key, Loc::Frame(slot));
        Ok(())
    }

    /// From here on no spill slot that is released is handed out again
    /// (§4.3), as for a call of a `returns_twice` function: for a target
    /// lowering whose code after it can run a second time and read a
    /// spill slot again (x86-64's `llvm.eh.sjlj.setjmp`).
    pub fn never_reuse_spill_slots(&mut self) {
        self.code_runs_twice();
        self.s.frame.keep_spills();
    }

    /// Code that can run a second time is being emitted (§4.3, §5.1): a
    /// function that gave out block slots is compiled again without.
    pub(crate) fn code_runs_twice(&mut self) {
        if self.block_slots {
            self.runs_twice = true;
        }
    }

    /// Displaces every register that holds a value and is not pinned.
    ///
    /// # Errors
    /// `limit:frame-size`.
    pub fn displace_all(&mut self) -> CgResult<()> {
        for reg in self.s.occupied.difference(self.s.pinned).iter() {
            self.displace(reg)?;
        }
        Ok(())
    }

    /// The lowest-numbered scratch register of `class` (`Gpr` or
    /// `Fpr`): what the policy itself passes to glue. A lowering may use
    /// the scratch registers freely *between* glue calls; glue
    /// overwrites them.
    ///
    /// # Panics
    /// Never for a `RegInfo` that passes [`RegInfo::check`](crate::RegInfo::check)
    /// and a class other than `Mem`; `Mem` yields the general one.
    #[inline]
    pub fn scratch_reg(&self, class: Class) -> PReg {
        let of_class = if class == Class::Fpr { self.ri.fpr } else { self.ri.gpr };
        self.ri.scratch.intersection(of_class).lowest().unwrap_or(self.ri.callee)
    }
}
