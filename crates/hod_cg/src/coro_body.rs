//! Coroutine bodies, the target-independent part (`spec/codegen.md` §7.6,
//! design 0005 §3.2 to §3.4, WP-106).
//!
//! A body with `presplitcoroutine` is compiled **once**, with its frame in
//! the heap: the plan (`hod_xform::coro::plan`) says which slots and which
//! values live there, and this module places them at [`FrameRef::Coro`]
//! offsets, which the target addresses from a reserved base register.
//! Every activation (the ramp, and each resumption through the target's
//! entry stubs) builds the same machine frame, so the code after a suspend
//! point is entered like the return point of a call: no register holds a
//! value there.
//!
//! This module owns:
//!
//! - the heap layout: the 16-byte header (resume and destroy pointers),
//!   the promise at `promise_offset`, the 4-byte suspend index after it,
//!   then the heap slots in slot order, then the heap values in
//!   instruction order, each at its own alignment;
//! - the two machine-stack words every activation has (the mode, 0 in
//!   the ramp and 1 in a resumed activation, and the frame address, which
//!   is where `coro.begin`'s result lives);
//! - the shadows of the `coro.begin` copies, switched per block: a block
//!   before `coro.begin` addresses the shadow, any other the heap;
//! - the re-entry label of each suspend, and the suspend `switch`, whose
//!   successors' `phi` slots are written per edge after the dispatch
//!   (§4.5's exception);
//! - the structural check of design 0005 §3.3: a heap access formed in
//!   the suspended region, or before `coro.begin`, is a
//!   [`CgError::Internal`] at the end of the function.
//!
//! The target lowers the intrinsics and writes the stubs
//! (`Target::coro_stubs`, `Target::coro_dispatch`).

use hod_ir::{BlockId, Const, Inst, InstId, InstKind, Intrinsic, SlotId, TypeId, ValueKind};
use hod_xform::coro::CoroPlan;

use crate::cx::{FnCx, TailExit};
use crate::error::{CgError, CgResult, LIMIT_FRAME_SIZE, internal};
use crate::frame::{FrameRef, SlotPlace};
use crate::label::Label;
use crate::loc::{Class, Key, Kind, Loc, NO_SLOT, ValState};
use crate::target::{CallTarget, Target};

/// A coroutine body intrinsic (`llvm.coro.begin`, `suspend`, ...) in a
/// function without `presplitcoroutine`.
pub const OUTSIDE_COROUTINE: &str = "coro:outside-coroutine";
/// A coroutine on a target that has no coroutine bodies
/// (`Target::CORO_BODIES`).
pub const CORO_TARGET: &str = "coro:target";
/// A block of a coroutine that runs both before and after `coro.begin`
/// and names a slot or a value copied at `coro.begin`: it would address
/// the shadow on one path and the heap on the other.
pub const BEGIN_SHARED_BLOCK: &str = "coro:begin-shared-block";
/// A stack slot aligned above the stack alignment that is not in the
/// heap frame: its memory is allocated by the ramp's entry code only.
pub const OVERALIGNED_STACK: &str = "coro:overaligned-stack-slot";

/// "Not in the heap."
const NO_HEAP: u32 = u32::MAX;

/// The machine-stack words every activation of a coroutine has
/// ([`FnCx::coro_slots`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoroSlots {
    /// 0 in the ramp, 1 in an activation a stub entered (8 bytes).
    pub mode: FrameRef,
    /// The frame's address: `coro.begin`'s result (8 bytes).
    pub frame: FrameRef,
}

/// The heap frame's layout ([`FnCx::coro_layout`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoroLayout {
    /// What `llvm.coro.size` returns.
    pub size: u32,
    /// What `llvm.coro.align` returns: at least 16.
    pub align: u32,
    /// The offset of the 4-byte suspend index.
    pub index: u32,
    /// The final suspend's index, if there is one.
    pub final_index: Option<u32>,
}

/// One suspend point as the emitter needs it.
#[derive(Clone, Copy, Debug)]
struct Point {
    save: InstId,
    wrapper: Option<InstId>,
    suspend: InstId,
    index: u32,
    is_final: bool,
    reentry: Label,
}

/// The per-function state of a coroutine body (or of a function forced
/// through the coroutine frame by `Stress::coro_frame`, which has no
/// plan).
#[derive(Debug)]
pub(crate) struct CoroFn {
    plan: Option<CoroPlan>,
    slots: Option<CoroSlots>,
    layout: CoroLayout,
    /// By `SlotId`: the heap offset, or [`NO_HEAP`].
    heap_slot: Vec<u32>,
    /// The `coro.begin` copies of slots: slot, shadow, heap place, bytes.
    slot_copies: Vec<(SlotId, FrameRef, FrameRef, u32)>,
    /// The `coro.begin` copies of values: value, shadow, heap home, bytes.
    value_copies: Vec<(InstId, FrameRef, FrameRef, u32)>,
    /// By `BlockId`: whether the block runs before `coro.begin`.
    pre_begin: Vec<bool>,
    points: Vec<Point>,
    /// Whether the code being emitted is in the suspended region.
    region: bool,
    /// Whether the code being emitted runs before `coro.begin`.
    before_begin: bool,
    violation: Option<String>,
    /// The stubs' offsets: resume, destroy, the end of the stubs.
    entries: Option<[u32; 3]>,
    /// Where each stub's frame is built (WP-108, [`FnCx::coro_stub_ready`]).
    ready: Vec<u32>,
}

impl CoroFn {
    fn stress() -> CoroFn {
        CoroFn {
            plan: None,
            slots: None,
            layout: CoroLayout { size: 0, align: 16, index: 0, final_index: None },
            heap_slot: Vec::new(),
            slot_copies: Vec::new(),
            value_copies: Vec::new(),
            pre_begin: Vec::new(),
            points: Vec::new(),
            region: false,
            before_begin: false,
            violation: None,
            entries: None,
            ready: Vec::new(),
        }
    }
}

/// Whether `block` names a slot or a value that is copied at
/// `coro.begin`: it would address the shadow on one path and the heap on
/// the other.
fn mentions_copy(body: &hod_ir::Body, block: BlockId, plan: &CoroPlan) -> bool {
    body.block(block).insts().any(|inst| {
        inst.operands().iter().any(|value| match value.kind() {
            ValueKind::Slot(slot) => {
                plan.promise == Some(slot) || plan.begin_copies.binary_search(&slot).is_ok()
            }
            ValueKind::Inst(id) => plan.begin_value_copies.binary_search(&id).is_ok(),
            _ => false,
        })
    })
}

/// The blocks before `coro.begin`: reachable from the entry block without
/// leaving `coro.begin`'s block. A block that also runs after it (the
/// ramp's return, reached from a failed allocation too) is refused when
/// it names a copied slot or value; otherwise it keeps `before_begin`,
/// and a heap access there is an internal error.
fn pre_begin(body: &hod_ir::Body, plan: &CoroPlan) -> CgResult<Vec<bool>> {
    let blocks = body.block_count();
    let begin_raw = plan.begin.raw();
    let Some(begin_block) = body.blocks().find(|b| b.inst_range().contains(&begin_raw)) else {
        return internal("coro.begin is in no block");
    };
    let begin_block = begin_block.id();
    let mut pre = vec![false; blocks];
    let mut work = vec![body.entry()];
    pre[body.entry().index()] = true;
    while let Some(block) = work.pop() {
        if block == begin_block {
            continue;
        }
        for succ in body.block(block).terminator().successors() {
            if !pre[succ.index()] {
                pre[succ.index()] = true;
                work.push(succ);
            }
        }
    }
    let mut after = vec![false; blocks];
    work.extend(body.block(begin_block).terminator().successors());
    while let Some(block) = work.pop() {
        if after[block.index()] {
            continue;
        }
        after[block.index()] = true;
        if pre[block.index()] && mentions_copy(body, block, plan) {
            return Err(CgError::unsupported(BEGIN_SHARED_BLOCK));
        }
        work.extend(body.block(block).terminator().successors());
    }
    Ok(pre)
}

/// Whether `inst` calls one of the intrinsics only a coroutine body may
/// have.
fn body_intrinsic(inst: Inst<'_>) -> Option<Intrinsic<'_>> {
    use Intrinsic as I;
    let which = inst.call()?.intrinsic()?;
    matches!(
        which,
        I::CoroId
            | I::CoroAlloc
            | I::CoroBegin
            | I::CoroEnd
            | I::CoroFree
            | I::CoroSize
            | I::CoroAlign
            | I::CoroSave
            | I::CoroSuspend
            | I::CoroFrame
            | I::CoroIsInRamp
            | I::CoroAwaitSuspendVoid
            | I::CoroAwaitSuspendBool
            | I::CoroAwaitSuspendHandle
    )
    .then_some(which)
}

impl<'a, T: Target> FnCx<'a, T> {
    // ---- for the driver ----

    /// Whether the plan puts body slot `index` in the heap frame (the
    /// promise or a heap slot): known from `coro_plan` on, before the
    /// heap layout (WP-183, §5.6.24: such a slot is not guarded).
    pub(crate) fn coro_heap_planned(&self, index: usize) -> bool {
        let Some(plan) = self.coro.as_deref().and_then(|coro| coro.plan.as_ref()) else {
            return false;
        };
        let id = SlotId::new(index as u32);
        plan.promise == Some(id) || plan.heap_slots.contains(id)
    }

    /// Before `begin_function`: the plan, if the body is a coroutine.
    pub(crate) fn coro_plan(&mut self) -> CgResult<()> {
        self.coro = None;
        match hod_xform::coro::plan(self.module, self.body) {
            Err(refusal) => return Err(CgError::unsupported(refusal.construct)),
            Ok(Some(plan)) => {
                if !T::CORO_BODIES {
                    return Err(CgError::unsupported(CORO_TARGET));
                }
                let mut coro = CoroFn::stress();
                coro.plan = Some(plan);
                self.coro = Some(Box::new(coro));
            }
            Ok(None) => {
                if self.body.insts().any(|inst| body_intrinsic(inst).is_some()) {
                    return Err(CgError::unsupported(OUTSIDE_COROUTINE));
                }
                if self.options.stress.coro_frame && T::CORO_BODIES {
                    self.coro = Some(Box::new(CoroFn::stress()));
                }
            }
        }
        Ok(())
    }

    /// After `begin_function`, before the slots are laid out: the heap
    /// layout, the locations of the heap values, the shadows.
    pub(crate) fn coro_setup(&mut self) -> CgResult<()> {
        let Some(mut coro) = self.coro.take() else { return Ok(()) };
        let result = self.setup(&mut coro);
        self.coro = Some(coro);
        result
    }

    fn setup(&mut self, coro: &mut CoroFn) -> CgResult<()> {
        let Some(plan) = coro.plan.as_ref() else { return Ok(()) };
        let Some(slots) = coro.slots else {
            return internal("a coroutine whose target allocated no coroutine slots");
        };
        let body = self.body;
        let types = self.types();
        let too_big = || CgError::unsupported(LIMIT_FRAME_SIZE);
        // The header, the promise, the index.
        let mut end = u64::from(plan.promise_offset);
        if let Some(promise) = plan.promise {
            end += body.slots().get(promise.index()).map_or(0, |slot| slot.size());
        }
        let index = end.next_multiple_of(4);
        end = index + 4;
        let mut place = |size: u64, align: u64| -> CgResult<u32> {
            let at = end.next_multiple_of(align.max(1));
            end = at.checked_add(size).filter(|&e| e < 1 << 31).ok_or_else(too_big)?;
            Ok(at as u32)
        };
        // The heap slots, in slot order.
        coro.heap_slot.clear();
        coro.heap_slot.resize(body.slots().len(), NO_HEAP);
        for (raw, slot) in body.slots().iter().enumerate() {
            let id = SlotId::new(raw as u32);
            if plan.promise == Some(id) {
                coro.heap_slot[raw] = plan.promise_offset;
            } else if plan.heap_slots.contains(id) {
                coro.heap_slot[raw] = place(slot.size(), slot.align())?;
            }
        }
        // The heap values, in instruction order; then the tokens, which
        // live nowhere, and `coro.begin`'s result, which is the frame
        // address word of the machine stack.
        for inst in body.insts() {
            let id = inst.id();
            let key = Key::inst(id.raw());
            let ty = inst.ty();
            if ty == TypeId::TOKEN {
                self.preset(key, Kind::Dead, Loc::None, Class::Gpr, 0);
                continue;
            }
            if id == plan.begin {
                self.preset(key, Kind::Home, Loc::Frame(slots.frame), Class::Gpr, 8);
                continue;
            }
            if !plan.heap_values.contains(id) {
                continue;
            }
            let class = self.target.class_of(types, ty);
            let (size, align) = if class == Class::Mem {
                (types.alloc_size_of(ty), types.align_of(ty).min(16))
            } else {
                let bytes = u64::from(self.reg_bytes(class));
                (bytes, bytes)
            };
            let heap = FrameRef::Coro(place(size, align)?);
            let width = self.reg_bytes(class) as u8;
            if plan.begin_value_copies.binary_search(&id).is_ok() {
                let shadow = self.value_slot(class, ty)?;
                let bytes = u32::try_from(size).map_err(|_| too_big())?;
                coro.value_copies.push((id, shadow, heap, bytes));
                self.preset(key, Kind::Home, Loc::Frame(shadow), class, width);
            } else {
                self.preset(key, Kind::Home, Loc::Frame(heap), class, width);
            }
        }
        let size = u32::try_from(end).map_err(|_| too_big())?;
        coro.layout = CoroLayout {
            size,
            align: plan.frame_align.max(16),
            index: index as u32,
            final_index: plan.final_index,
        };
        // The suspend points.
        coro.points.clear();
        for suspend in &plan.suspends {
            let reentry = self.new_label();
            coro.points.push(Point {
                save: suspend.save,
                wrapper: suspend.wrapper,
                suspend: suspend.suspend,
                index: suspend.index,
                is_final: suspend.is_final,
                reentry,
            });
        }
        coro.pre_begin = pre_begin(body, plan)?;
        Ok(())
    }

    fn preset(&mut self, key: Key, kind: Kind, loc: Loc, class: Class, width: u8) {
        if let Some(state) = self.state_mut(key) {
            *state = ValState { loc, kind, class, width, incoming: NO_SLOT };
        }
    }

    /// The place of body slot `index` in a coroutine (`layout_slots`):
    /// in the heap, or its shadow for one copied at `coro.begin`; `None`
    /// for a slot on the machine stack.
    pub(crate) fn coro_slot_place(&mut self, index: usize) -> CgResult<Option<SlotPlace>> {
        let Some(coro) = self.coro.as_deref() else { return Ok(None) };
        let Some(plan) = coro.plan.as_ref() else { return Ok(None) };
        let Some(&slot) = self.body.slots().get(index) else {
            return internal("a slot the body does not have");
        };
        let at = coro.heap_slot.get(index).copied().unwrap_or(NO_HEAP);
        let id = SlotId::new(index as u32);
        if at == NO_HEAP {
            if slot.align() > u64::from(T::STACK_ALIGN) {
                return Err(CgError::unsupported(OVERALIGNED_STACK));
            }
            return Ok(None);
        }
        let heap = FrameRef::Coro(at);
        // The promise has a shadow too: `coro.id` names its address
        // before `coro.begin` (LLVM 19 through a `bitcast`).
        if plan.promise != Some(id) && plan.begin_copies.binary_search(&id).is_err() {
            return Ok(Some(SlotPlace::Direct(heap)));
        }
        let align = slot.align().min(u64::from(T::STACK_ALIGN)) as u32;
        let shadow = self.s.frame.alloc(slot.size(), align)?;
        let bytes =
            u32::try_from(slot.size()).map_err(|_| CgError::unsupported(LIMIT_FRAME_SIZE))?;
        if let Some(coro) = self.coro.as_deref_mut() {
            coro.slot_copies.push((id, shadow, heap, bytes));
        }
        Ok(Some(SlotPlace::Direct(shadow)))
    }

    /// At the start of each block: whether it is in the suspended region
    /// or before `coro.begin`, and the shadows or the heap accordingly.
    pub(crate) fn coro_block_start(&mut self, block: BlockId) {
        let Some(coro) = self.coro.as_deref_mut() else { return };
        let Some(plan) = coro.plan.as_ref() else { return };
        coro.region = plan.suspended_region.contains(block);
        coro.before_begin = coro.pre_begin.get(block.index()).copied().unwrap_or(false);
        let before = coro.before_begin;
        self.coro_switch_shadows(before);
    }

    fn coro_switch_shadows(&mut self, before: bool) {
        let Some(coro) = self.coro.take() else { return };
        for &(slot, shadow, heap, _) in &coro.slot_copies {
            if let Some(place) = self.s.slots.get_mut(slot.index()) {
                *place = SlotPlace::Direct(if before { shadow } else { heap });
            }
        }
        for &(id, shadow, heap, _) in &coro.value_copies {
            self.set_loc(Key::inst(id.raw()), Loc::Frame(if before { shadow } else { heap }));
        }
        self.coro = Some(coro);
    }

    /// Lowers a terminator that is the `switch` on a `coro.suspend`
    /// result, and returns `true`; `false` for any other instruction
    /// (design 0005 §3.3): the dispatch first, then each edge's incoming
    /// slots on that edge, the −1 edge in the suspended region and the 0
    /// and 1 edges outside it.
    pub(crate) fn coro_switch(&mut self, inst: Inst<'a>) -> CgResult<bool> {
        let Some(coro) = self.coro.as_deref() else { return Ok(false) };
        let InstKind::Switch(switch) = inst.kind() else { return Ok(false) };
        let Some(cond) = switch.cond.as_inst() else { return Ok(false) };
        if !coro.points.iter().any(|point| point.suspend == cond) {
            return Ok(false);
        }
        let (mut resume, mut destroy) = (None, None);
        for (case, block) in switch.cases() {
            match self.module.consts.get(case) {
                Const::Int { value: 0, .. } => resume = resume.or(Some(block)),
                Const::Int { value: 1, .. } => destroy = destroy.or(Some(block)),
                _ => return self.bug("a suspend switch with a case other than 0 and 1"),
            }
        }
        let (Some(resume), Some(destroy)) = (resume, destroy) else {
            return self.bug("a suspend switch without its cases 0 and 1");
        };
        let t = self.target;
        let reg = self.use_reg(switch.cond)?;
        let (l0, l1) = (self.new_label(), self.new_label());
        t.coro_dispatch(self, reg, l0, l1)?;
        // The −1 edge: the activation that suspended.
        self.set_region(true);
        self.phi_stores_edge(switch.default)?;
        t.jump(self, self.block_label(switch.default))?;
        // The 0 and 1 edges: a resumed activation.
        self.set_region(false);
        self.bind(l0)?;
        self.phi_stores_edge(resume)?;
        t.jump(self, self.block_label(resume))?;
        self.bind(l1)?;
        self.phi_stores_edge(destroy)?;
        if !self.falls_through(destroy) {
            t.jump(self, self.block_label(destroy))?;
        }
        Ok(true)
    }

    fn set_region(&mut self, on: bool) {
        if let Some(coro) = self.coro.as_deref_mut() {
            coro.region = on;
        }
    }

    /// From the call sequence, right after the call of `inst` returned:
    /// an `await_suspend` wrapper's return starts the suspended region;
    /// after the wrapper of `llvm.coro.await.suspend.handle`, the
    /// symmetric transfer (WP-107, [`Target::coro_transfer`]).
    ///
    /// # Errors
    /// What the target's transfer reports.
    pub(crate) fn coro_called(&mut self, inst: Inst<'a>) -> CgResult<()> {
        let id = inst.id();
        let Some(coro) = self.coro.as_deref_mut() else { return Ok(()) };
        if coro.points.iter().any(|point| point.wrapper == Some(id)) {
            coro.region = true;
        }
        let handle = inst.call().and_then(|site| site.intrinsic());
        if matches!(handle, Some(Intrinsic::CoroAwaitSuspendHandle)) {
            let exit = self.coro_tail_exit();
            let t = self.target;
            t.coro_transfer(self, exit)?;
        }
        Ok(())
    }

    /// A tail exit for symmetric transfer (WP-107): `end_function` binds
    /// the returned label at an exit sequence that tears the machine
    /// frame down (restoring the frame base register) and jumps to the
    /// callee register, where [`Target::coro_transfer`] has loaded the
    /// resume function; the argument registers are left as they are.
    pub fn coro_tail_exit(&mut self) -> Label {
        let label = self.new_label();
        let target = CallTarget::Reg(self.ri.callee);
        self.s.tail_exits.push(TailExit { label, target });
        label
    }

    /// After `end_function`: the stubs, and the structural check.
    pub(crate) fn coro_finish(&mut self) -> CgResult<()> {
        let Some(coro) = self.coro.as_deref() else { return Ok(()) };
        if coro.plan.is_none() {
            return Ok(());
        }
        let t = self.target;
        t.coro_stubs(self)?;
        let Some(coro) = self.coro.as_deref() else { return Ok(()) };
        if let Some(violation) = &coro.violation {
            return internal(violation.clone());
        }
        if coro.entries.is_none() {
            return internal("a coroutine without its entry stubs");
        }
        Ok(())
    }

    /// The stubs' offsets for the function's record.
    pub(crate) fn coro_entries(&self) -> Option<[u32; 3]> {
        self.coro.as_deref().and_then(|coro| coro.entries)
    }

    // ---- for the target ----

    /// Whether the function is a coroutine body (it has a plan).
    #[inline]
    pub fn is_coroutine(&self) -> bool {
        self.coro.as_deref().is_some_and(|coro| coro.plan.is_some())
    }

    /// Whether the function has the coroutine machine frame: it is a
    /// coroutine, or `Stress::coro_frame` forces the frame on it.
    #[inline]
    pub fn has_coro_frame(&self) -> bool {
        self.coro.is_some()
    }

    /// The machine-stack words of a function with the coroutine frame,
    /// allocated at the first call (from `begin_function`, after the
    /// target's own slots).
    ///
    /// # Errors
    /// `Internal` for a function without the coroutine frame; what
    /// allocating reports.
    pub fn coro_slots(&mut self) -> CgResult<CoroSlots> {
        if let Some(slots) = self.coro.as_deref().and_then(|coro| coro.slots) {
            return Ok(slots);
        }
        if self.coro.is_none() {
            return internal("coroutine slots in a function without the coroutine frame");
        }
        let mode = self.s.frame.alloc(8, 8)?;
        let frame = self.s.frame.alloc(8, 8)?;
        let slots = CoroSlots { mode, frame };
        if let Some(coro) = self.coro.as_deref_mut() {
            coro.slots = Some(slots);
        }
        Ok(slots)
    }

    /// The heap frame's layout.
    ///
    /// # Errors
    /// `Internal` outside a coroutine.
    pub fn coro_layout(&self) -> CgResult<CoroLayout> {
        match self.coro.as_deref() {
            Some(coro) if coro.plan.is_some() => Ok(coro.layout),
            _ => self.bug("a coroutine intrinsic outside a coroutine"),
        }
    }

    /// Records a heap access being formed: the target calls this for
    /// every [`FrameRef::Coro`] it addresses. One formed in the suspended
    /// region or before `coro.begin` makes the function an internal
    /// error (design 0005 §3.3).
    #[inline]
    pub fn coro_access(&mut self) {
        let Some(coro) = self.coro.as_deref_mut() else { return };
        if (coro.region || coro.before_begin) && coro.violation.is_none() {
            let what = if coro.region { "in the suspended region" } else { "before coro.begin" };
            coro.violation = Some(format!(
                "a coroutine frame access {what} (at {:?} = {})",
                self.cur,
                self.body.inst(self.cur).opcode().name()
            ));
        }
    }

    /// From the lowering of `coro.begin`, once the frame base is set:
    /// the shadows are copied to the heap, and from here on the heap is
    /// what the body addresses.
    ///
    /// # Errors
    /// `Internal` outside a coroutine.
    pub fn coro_began(&mut self) -> CgResult<()> {
        let Some(coro) = self.coro.as_deref_mut().filter(|coro| coro.plan.is_some()) else {
            return self.bug("coro.begin outside a coroutine");
        };
        coro.before_begin = false;
        let slots = coro.slot_copies.iter().map(|&(_, shadow, heap, bytes)| (shadow, heap, bytes));
        let values =
            coro.value_copies.iter().map(|&(_, shadow, heap, bytes)| (shadow, heap, bytes));
        let copies: Vec<(FrameRef, FrameRef, u32)> = slots.chain(values).collect();
        let t = self.target;
        for (shadow, heap, bytes) in copies {
            if bytes != 0 {
                t.copy(self, heap, shadow, bytes);
            }
        }
        self.coro_switch_shadows(false);
        Ok(())
    }

    /// The suspend point whose `coro.save` is `inst`: its index and
    /// whether it is the final one. In the older dialect (no wrapper)
    /// the suspended region starts here; the target calls this after
    /// its stores.
    ///
    /// # Errors
    /// `Internal` for a save the plan does not know.
    pub fn coro_save(&mut self, inst: Inst<'a>) -> CgResult<(u32, bool)> {
        let id = inst.id();
        let found = self
            .coro
            .as_deref()
            .and_then(|coro| coro.points.iter().find(|point| point.save == id).copied());
        match found {
            Some(point) => Ok((point.index, point.is_final)),
            None => self.bug("a coro.save the plan does not know"),
        }
    }

    /// After the stores of a `coro.save`: the older dialect's suspended
    /// region starts.
    pub fn coro_saved(&mut self, inst: Inst<'a>) {
        let id = inst.id();
        if let Some(coro) = self.coro.as_deref_mut()
            && coro.points.iter().any(|point| point.save == id && point.wrapper.is_none())
        {
            coro.region = true;
        }
    }

    /// The re-entry label of `coro.suspend` `inst`, which the target binds
    /// right after the instruction that sets the −1 result. No register
    /// may hold a value here: a resumed activation enters at the label.
    ///
    /// # Errors
    /// `Internal` for a suspend the plan does not know, or a register
    /// value live across it.
    pub fn coro_reentry(&mut self, inst: Inst<'a>) -> CgResult<Label> {
        if !self.s.occupied.is_empty() {
            return self.bug("a register value live across a coro.suspend");
        }
        let id = inst.id();
        let found = self
            .coro
            .as_deref()
            .and_then(|coro| coro.points.iter().find(|point| point.suspend == id).copied());
        let Some(point) = found else {
            return self.bug("a coro.suspend the plan does not know");
        };
        self.set_region(true);
        Ok(point.reentry)
    }

    /// The re-entry points, as `(index, label)`, for the stubs'
    /// dispatch.
    pub fn coro_reentries(&self) -> Vec<(u32, Label)> {
        self.coro
            .as_deref()
            .map(|coro| coro.points.iter().map(|point| (point.index, point.reentry)).collect())
            .unwrap_or_default()
    }

    /// Records where the stubs are: the resume stub's offset, the destroy
    /// stub's, and the end of the stubs (`Target::coro_stubs`).
    pub fn set_coro_entries(&mut self, resume: u32, destroy: u32, end: u32) {
        if let Some(coro) = self.coro.as_deref_mut() {
            coro.entries = Some([resume, destroy, end]);
        }
    }

    // ---- debug information (WP-108, `coro_debug`) ----

    /// What the variable locations of a coroutine body need: the machine
    /// words, and the `coro.begin` copies as (shadow, heap place) of
    /// slots and of values. `None` outside a coroutine.
    pub(crate) fn coro_debug_view(&self) -> Option<crate::coro_debug::View> {
        let coro = self.coro.as_deref()?;
        coro.plan.as_ref()?;
        let slots = coro.slots?;
        let pairs = |copies: &mut dyn Iterator<Item = (FrameRef, FrameRef)>| copies.collect();
        Some(crate::coro_debug::View {
            words: slots,
            slot_copies: pairs(&mut coro.slot_copies.iter().map(|&(_, s, h, _)| (s, h))),
            value_copies: pairs(&mut coro.value_copies.iter().map(|&(_, s, h, _)| (s, h))),
        })
    }

    /// From the target's stubs, once a stub's machine frame, mode and
    /// frame word are set: where a debugger stepping into the stub stops
    /// (its `prologue_end`, `spec/dwarf.md` §6.16). Once per stub, in
    /// stub order.
    pub fn coro_stub_ready(&mut self) {
        let offset = self.offset();
        if let Some(coro) = self.coro.as_deref_mut() {
            coro.ready.push(offset);
        }
    }

    /// Moves the stubs' offsets with the code (`shrink`, §5.6.15).
    pub(crate) fn map_coro_offsets(&mut self, map: impl Fn(u32) -> u32) {
        if let Some(coro) = self.coro.as_deref_mut() {
            if let Some(entries) = &mut coro.entries {
                *entries = entries.map(&map);
            }
            for offset in &mut coro.ready {
                *offset = map(*offset);
            }
        }
    }

    /// Where each stub's frame is built, for the function's record.
    pub(crate) fn coro_ready(&self) -> Option<[u32; 2]> {
        match self.coro.as_deref()?.ready.as_slice() {
            &[resume, destroy] => Some([resume, destroy]),
            _ => None,
        }
    }
}
