//! The plan's analysis (`spec/xform.md` §3).

use hod_ir::{
    BlockId, Body, FnAttrs, InstId, InstKind, Intrinsic, Module, Opcode, SlotId, SymbolKind,
    TypeId, TypeKind, Value, ValueKind,
};

use super::{
    ARGUMENT_ACROSS_SUSPEND, Analysis, BEGIN_SHAPE, CoroPlan, DYNAMIC_ALLOCA, FRAME_BEFORE_BEGIN,
    FRAME_INTRINSIC, OUTSIDE_FRAME_ACROSS_SUSPEND, Refusal, Region, SUSPEND_SHAPE, SlotClass,
    Suspend, UNWIND_END_WITHOUT_FINAL, VARIADIC, Violation,
};
use crate::bitset::BitSet;

const NONE: u32 = u32::MAX;

fn refuse<T>(construct: &'static str, inst: Option<InstId>) -> Result<T, Refusal> {
    Err(Refusal { construct, inst, slot: None })
}

/// What an edge is, for the activation that takes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Edge {
    Normal,
    /// The 0 or 1 arm of a suspend's switch: taken only in a resumed
    /// activation, which starts there.
    Resume,
    /// The default (−1) arm of a suspend's switch.
    Suspended,
    /// Taken only in the ramp: the true side of `is_in_ramp`, the false
    /// side of LLVM 19's unwinding `i1 coro.end`.
    RampOnly,
}

/// How a block ends, for the edges out of it.
#[derive(Clone, Copy, Debug)]
enum Term {
    Plain,
    /// The switch on suspend `n`'s result (index into `Coro::suspends`).
    Suspend(u32),
    /// A conditional branch whose `ramp` side only the ramp takes.
    Ramp {
        ramp: BlockId,
        other: BlockId,
    },
}

/// The control flow, as the plan reads it.
struct Cfg<'b> {
    body: &'b Body,
    /// Per block: how it ends.
    term: Vec<Term>,
    /// The blocks reachable from the entry, in reverse post-order.
    rpo: Vec<BlockId>,
    /// `(first instruction, block)` in layout order.
    starts: Vec<(u32, BlockId)>,
}

impl Cfg<'_> {
    fn block_of(&self, inst: u32) -> BlockId {
        let at = self.starts.partition_point(|&(first, _)| first <= inst);
        self.starts[at.saturating_sub(1)].1
    }

    /// The `k`-th successor of `block` in the order of [`Cfg::edges`].
    fn edge_at(&self, block: BlockId, k: usize, suspends: &[RawSuspend]) -> Option<BlockId> {
        match self.term[block.index()] {
            Term::Plain => self.body.block(block).successors().nth(k),
            Term::Suspend(n) => {
                let s = &suspends[n as usize];
                [s.suspended, s.resume, s.destroy].get(k).copied()
            }
            Term::Ramp { ramp, other } => [ramp, other].get(k).copied(),
        }
    }

    /// The edges out of `block`, each with its kind; an edge listed twice
    /// is visited twice.
    fn edges(&self, block: BlockId, suspends: &[RawSuspend], mut f: impl FnMut(BlockId, Edge)) {
        match self.term[block.index()] {
            Term::Plain => {
                for succ in self.body.block(block).successors() {
                    f(succ, Edge::Normal);
                }
            }
            Term::Suspend(n) => {
                let s = &suspends[n as usize];
                f(s.suspended, Edge::Suspended);
                f(s.resume, Edge::Resume);
                f(s.destroy, Edge::Resume);
            }
            Term::Ramp { ramp, other } => {
                f(ramp, Edge::RampOnly);
                f(other, Edge::Normal);
            }
        }
    }
}

/// A suspend point as found.
#[derive(Clone, Copy, Debug)]
struct RawSuspend {
    inst: u32,
    block: BlockId,
    save: u32,
    is_final: bool,
    wrapper: u32,
    suspended: BlockId,
    resume: BlockId,
    destroy: BlockId,
}

/// The coroutine intrinsics of a body.
#[derive(Default)]
struct Coro {
    ids: Vec<u32>,
    begins: Vec<u32>,
    saves: Vec<u32>,
    suspends: Vec<RawSuspend>,
    /// `(call, is .bool)`.
    wrappers: Vec<(u32, bool)>,
    /// `(call, unwinds)`.
    ends: Vec<(u32, bool)>,
    in_ramp: Vec<u32>,
    frees: Vec<u32>,
    calls_align: bool,
    frame: Option<u32>,
    stack: Option<u32>,
}

/// The derived addresses (`getelementptr`, `bitcast`, `addrspacecast`),
/// by instruction, and the slot each is an alias of.
struct Aliases {
    /// The derived instructions, increasing.
    ids: Vec<u32>,
    roots: Vec<Option<Root>>,
}

impl Aliases {
    fn get(&self, inst: u32) -> Option<Root> {
        self.ids.binary_search(&inst).ok().and_then(|k| self.roots[k])
    }
}

/// What a value or slot address refers to: a slot, at a known byte
/// offset or not.
#[derive(Clone, Copy, Debug)]
struct Root {
    slot: SlotId,
    offset: Option<i64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum UseKind {
    /// A load, a store to it, a `memcpy`/`memmove`/`memset` on it.
    Access,
    /// Anything that publishes the address.
    Escape,
    /// A `getelementptr` or cast that derives an alias.
    Derive,
    /// A lifetime marker: no code.
    Marker,
}

/// One use of a slot, through an alias or not.
#[derive(Clone, Copy, Debug)]
struct SlotUse {
    slot: SlotId,
    inst: u32,
    block: BlockId,
    /// The alias it is made through; `NONE` for the slot's address.
    via: u32,
    kind: UseKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    /// A store of the whole object, or a definition: "stored" set,
    /// "crossed since" cleared.
    Def,
    /// A partial store, or a `lifetime.start`: "stored" set.
    Gen,
    /// A read (rule 3), any use (rule 4), a use (values): a hit if
    /// "crossed since" is set.
    Use,
    /// `coro.end(false)`: a resumed activation returns here.
    Cut,
}

#[derive(Clone, Copy, Debug)]
struct Event {
    pos: u32,
    track: u32,
    op: Op,
}

/// A use on an edge (a `phi`'s incoming value).
#[derive(Clone, Copy, Debug)]
struct EdgeUse {
    pred: BlockId,
    succ: BlockId,
    /// The track of an instruction result; `NONE` for an argument.
    track: u32,
}

/// The tracks of the dataflow: per slot one for rule 3 and one for rule
/// 4, then one per candidate value.
struct Tracks {
    slots: u32,
    /// Candidate values (instruction results used outside their block),
    /// increasing.
    values: Vec<u32>,
}

impl Tracks {
    fn rule3(slot: SlotId) -> u32 {
        slot.index() as u32
    }
    fn rule4(&self, slot: SlotId) -> u32 {
        self.slots + slot.index() as u32
    }
    fn value(&self, inst: u32) -> Option<u32> {
        self.values.binary_search(&inst).ok().map(|k| 2 * self.slots + k as u32)
    }
    fn len(&self) -> usize {
        2 * self.slots as usize + self.values.len()
    }
}

/// Whether the instruction result is something the code generator
/// recomputes rather than keeps: tokens, the frame address, constants.
fn recomputed(intrinsic: Option<Intrinsic<'_>>) -> bool {
    matches!(
        intrinsic,
        Some(
            Intrinsic::CoroId
                | Intrinsic::CoroSave
                | Intrinsic::CoroBegin
                | Intrinsic::CoroFrame
                | Intrinsic::CoroSize
                | Intrinsic::CoroAlign
                | Intrinsic::CoroAlloc
                | Intrinsic::CoroSuspend
                | Intrinsic::CoroIsInRamp
                | Intrinsic::CoroEnd
        )
    )
}

fn int_arg(module: &Module, value: Option<&Value>) -> Option<u64> {
    value.and_then(|v| v.as_const()).and_then(|id| module.consts.int_value(id))
}

fn inst_of(value: Value) -> Option<u32> {
    match value.kind() {
        ValueKind::Inst(id) => Some(id.raw()),
        _ => None,
    }
}

/// Plain reachability from `starts` over the edges `follow` accepts,
/// not entering `stop`.
fn reach(
    cfg: &Cfg<'_>,
    suspends: &[RawSuspend],
    starts: impl IntoIterator<Item = BlockId>,
    expand: impl Fn(BlockId) -> bool,
    follow: impl Fn(Edge) -> bool,
) -> BitSet<BlockId> {
    let mut seen = BitSet::new(cfg.body.block_count());
    let mut work: Vec<BlockId> = Vec::new();
    for start in starts {
        if seen.insert(start) {
            work.push(start);
        }
    }
    while let Some(block) = work.pop() {
        if !expand(block) {
            continue;
        }
        cfg.edges(block, suspends, |succ, edge| {
            if follow(edge) && seen.insert(succ) {
                work.push(succ);
            }
        });
    }
    seen
}

#[allow(clippy::too_many_lines, reason = "the steps of one analysis, in order")]
pub(super) fn analyze(module: &Module, body: &Body) -> Result<Option<Analysis>, Refusal> {
    // ---- the function ----
    let sym = body.symbol();
    if sym.index() >= module.symbol_count() {
        return Ok(None);
    }
    let SymbolKind::Function { attrs, .. } = module.symbol(sym).kind() else {
        return Ok(None);
    };
    if attrs.index() >= module.fn_attrs_count()
        || !module.fn_attrs(attrs).has(FnAttrs::PRESPLIT_COROUTINE)
    {
        return Ok(None);
    }
    if let TypeKind::Function { variadic: true, .. } = module.types.kind(body.fn_type()) {
        return refuse(VARIADIC, None);
    }

    // ---- layout ----
    let block_count = body.block_count();
    let mut starts = Vec::with_capacity(block_count);
    for block in body.blocks() {
        starts.push((block.inst_range().start, block.id()));
    }
    let mut cfg = Cfg { body, term: vec![Term::Plain; block_count], rpo: Vec::new(), starts };

    // ---- the coroutine intrinsics ----
    let mut coro = Coro::default();
    for call in body.calls() {
        let at = call.inst().raw();
        let args = call.args();
        match call.intrinsic() {
            Some(Intrinsic::CoroId) => coro.ids.push(at),
            Some(Intrinsic::CoroBegin) => coro.begins.push(at),
            Some(Intrinsic::CoroSave) => coro.saves.push(at),
            Some(Intrinsic::CoroSuspend) => {
                let save = args.first().copied().and_then(inst_of).unwrap_or(NONE);
                let is_final = int_arg(module, args.get(1)) == Some(1);
                coro.suspends.push(RawSuspend {
                    inst: at,
                    block: BlockId::new(0),
                    save,
                    is_final,
                    wrapper: NONE,
                    suspended: BlockId::new(0),
                    resume: BlockId::new(0),
                    destroy: BlockId::new(0),
                });
            }
            Some(Intrinsic::CoroAwaitSuspendVoid | Intrinsic::CoroAwaitSuspendHandle) => {
                coro.wrappers.push((at, false));
            }
            Some(Intrinsic::CoroAwaitSuspendBool) => coro.wrappers.push((at, true)),
            Some(Intrinsic::CoroEnd) => {
                coro.ends.push((at, int_arg(module, args.get(1)) == Some(1)));
            }
            Some(Intrinsic::CoroIsInRamp) => coro.in_ramp.push(at),
            Some(Intrinsic::CoroFree) => coro.frees.push(at),
            Some(Intrinsic::CoroAlign) => coro.calls_align = true,
            Some(Intrinsic::CoroFrame) => {
                coro.frame.get_or_insert(at);
            }
            Some(Intrinsic::StackSave | Intrinsic::StackRestore) => {
                coro.stack.get_or_insert(at);
            }
            _ => {}
        }
    }
    let (&[id], &[begin]) = (&coro.ids[..], &coro.begins[..]) else {
        return refuse(BEGIN_SHAPE, None);
    };
    if let Some(at) = coro.frame {
        return refuse(FRAME_INTRINSIC, Some(InstId::new(at)));
    }
    if let Some(at) = coro.stack {
        return refuse(DYNAMIC_ALLOCA, Some(InstId::new(at)));
    }

    // ---- the suspend points ----
    let is_save = |inst: u32| coro.saves.binary_search(&inst).is_ok();
    let mut finals = 0;
    for n in 0..coro.suspends.len() {
        let s = coro.suspends[n];
        let shape = Some(InstId::new(s.inst));
        if !is_save(s.save) || body.uses(InstId::new(s.inst)).count() != 1 {
            return refuse(SUSPEND_SHAPE, shape);
        }
        let next = InstId::new(s.inst + 1);
        if next.index() >= body.inst_count() {
            return refuse(SUSPEND_SHAPE, shape);
        }
        let InstKind::Switch(switch) = body.inst(next).kind() else {
            return refuse(SUSPEND_SHAPE, shape);
        };
        if inst_of(switch.cond) != Some(s.inst) || switch.case_count() != 2 {
            return refuse(SUSPEND_SHAPE, shape);
        }
        let (mut resume, mut destroy) = (None, None);
        for (value, target) in switch.cases() {
            match module.consts.int_value(value) {
                Some(0) => resume = Some(target),
                Some(1) => destroy = Some(target),
                _ => {}
            }
        }
        let (Some(resume), Some(destroy)) = (resume, destroy) else {
            return refuse(SUSPEND_SHAPE, shape);
        };
        let block = cfg.block_of(s.inst);
        cfg.term[block.index()] = Term::Suspend(n as u32);
        let s = &mut coro.suspends[n];
        s.block = block;
        s.suspended = switch.default;
        s.resume = resume;
        s.destroy = destroy;
        finals += u32::from(s.is_final);
    }
    if finals > 1 {
        return refuse(SUSPEND_SHAPE, None);
    }
    let has_unwind_end = coro.ends.iter().any(|&(_, unwinds)| unwinds);
    if has_unwind_end && finals == 0 {
        let at = coro.ends.iter().find(|&&(_, unwinds)| unwinds).map(|&(at, _)| InstId::new(at));
        return refuse(UNWIND_END_WITHOUT_FINAL, at);
    }

    // The ramp-only edges.
    for &at in &coro.in_ramp {
        ramp_branch(&mut cfg, at, true);
    }
    for &(at, unwinds) in &coro.ends {
        if unwinds && body.inst(InstId::new(at)).ty() == TypeId::I1 {
            ramp_branch(&mut cfg, at, false);
        }
    }

    // Each wrapper's path to its suspend (and in the older dialect each
    // save's): the suspended region's first part.
    let mut region = BitSet::new(block_count);
    let mut tails: Vec<u32> = vec![NONE; block_count];
    for &(wrapper, is_bool) in &coro.wrappers {
        let n = walk_to_suspend(&cfg, &coro.suspends, wrapper, is_bool, &mut region).ok_or(
            Refusal { construct: SUSPEND_SHAPE, inst: Some(InstId::new(wrapper)), slot: None },
        )?;
        let s = &mut coro.suspends[n];
        if s.wrapper != NONE {
            return refuse(SUSPEND_SHAPE, Some(InstId::new(wrapper)));
        }
        s.wrapper = wrapper;
        let block = cfg.block_of(wrapper).index();
        tails[block] = tails[block].min(wrapper);
    }
    for n in 0..coro.suspends.len() {
        let s = coro.suspends[n];
        if s.wrapper != NONE {
            continue;
        }
        let found = walk_to_suspend(&cfg, &coro.suspends, s.save, false, &mut region);
        if found != Some(n) {
            return refuse(SUSPEND_SHAPE, Some(InstId::new(s.save)));
        }
        let block = cfg.block_of(s.save).index();
        tails[block] = tails[block].min(s.save);
    }

    // Reverse post-order from the entry.
    cfg.rpo = reverse_post_order(&cfg, &coro.suspends);

    // `coro.begin` runs once, and what runs before it.
    let begin_block = cfg.block_of(begin);
    let again = reach(
        &cfg,
        &coro.suspends,
        successors(&cfg, &coro.suspends, begin_block),
        |_| true,
        |_| true,
    );
    if again.contains(begin_block) {
        return refuse(BEGIN_SHAPE, Some(InstId::new(begin)));
    }
    let pre_blocks =
        reach(&cfg, &coro.suspends, [body.entry()], |block| block != begin_block, |_| true);
    let pre_begin = |inst: u32, block: BlockId| {
        if block == begin_block { inst < begin } else { pre_blocks.contains(block) }
    };

    // The suspended region: every block reachable from a −1 edge.
    let suspended_starts: Vec<BlockId> = coro.suspends.iter().map(|s| s.suspended).collect();
    let minus_one = reach(&cfg, &coro.suspends, suspended_starts, |_| true, |_| true);
    for block in minus_one.iter() {
        region.insert(block);
    }
    let in_region = |inst: u32, block: BlockId| {
        region.contains(block) || (tails[block.index()] != NONE && inst > tails[block.index()])
    };

    // After a `coro.end(false)`, a resumed activation has returned.
    let mut cut_at: Vec<u32> = vec![NONE; block_count];
    for &(at, unwinds) in &coro.ends {
        if !unwinds {
            let block = cfg.block_of(at).index();
            cut_at[block] = cut_at[block].min(at);
        }
    }

    // ---- aliases, candidate values ----
    let slot_count = body.slots().len();
    let mut derived: Vec<(u32, Value, Option<i64>)> = Vec::new();
    let mut values: Vec<u32> = Vec::new();
    for block in body.blocks() {
        for inst in block.insts() {
            let raw = inst.id().raw();
            match inst.kind() {
                InstKind::PtrAdd(add) => {
                    let offset = (add.index_count() == 0).then_some(add.offset);
                    derived.push((raw, add.base, offset));
                }
                InstKind::Cast { arg }
                    if matches!(inst.opcode(), Opcode::BitCast | Opcode::AddrSpaceCast) =>
                {
                    derived.push((raw, arg, Some(0)));
                }
                InstKind::Alloca { .. } => return refuse(DYNAMIC_ALLOCA, Some(inst.id())),
                _ => {}
            }
            if inst.uses().used_outside_block()
                && !recomputed(inst.call().and_then(|call| call.intrinsic()))
            {
                values.push(raw);
            }
        }
    }
    // Aliases of aliases, whatever the layout order.
    derived.sort_unstable_by_key(|&(raw, ..)| raw);
    let mut aliases = Aliases {
        ids: derived.iter().map(|&(raw, ..)| raw).collect(),
        roots: vec![None; derived.len()],
    };
    let root_of = |aliases: &Aliases, value: Value| -> Option<Root> {
        match value.kind() {
            ValueKind::Slot(slot) => Some(Root { slot, offset: Some(0) }),
            ValueKind::Inst(id) => aliases.get(id.raw()),
            _ => None,
        }
    };
    loop {
        let mut changed = false;
        for (k, &(_, base, offset)) in derived.iter().enumerate() {
            if aliases.roots[k].is_some() {
                continue;
            }
            if let Some(root) = root_of(&aliases, base) {
                let offset = match (root.offset, offset) {
                    (Some(a), Some(b)) => a.checked_add(b),
                    _ => None,
                };
                aliases.roots[k] = Some(Root { slot: root.slot, offset });
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let tracks = Tracks { slots: slot_count as u32, values };

    // ---- the uses, and the events of the dataflow ----
    let types = &module.types;
    let slot_size = |slot: SlotId| body.slots()[slot.index()].size();
    let mut uses: Vec<SlotUse> = Vec::new();
    let mut escaped: BitSet<SlotId> = BitSet::new(slot_count);
    let mut markers: BitSet<SlotId> = BitSet::new(slot_count);
    let mut events: Vec<Event> = Vec::new();
    let mut event_start: Vec<u32> = vec![0; block_count + 1];
    let mut edge_uses: Vec<EdgeUse> = Vec::new();
    let mut arg_uses: Vec<(u32, BlockId)> = Vec::new();
    let all_markers = body.lifetime_markers();
    let mut next_marker = 0;
    let promise = {
        let id_call = body.inst(InstId::new(id));
        let operand = id_call.call().and_then(|call| call.args().get(1).copied());
        match operand {
            None => None,
            Some(value) if value.as_const().is_some() => None,
            Some(value) => match root_of(&aliases, value) {
                Some(Root { slot, offset: Some(0) }) => Some(slot),
                _ => return refuse(BEGIN_SHAPE, Some(InstId::new(id))),
            },
        }
    };

    let mut block_events: Vec<(BlockId, u32, u32)> = Vec::with_capacity(block_count);
    for block in body.blocks() {
        let id_b = block.id();
        let first = events.len() as u32;
        for inst in block.insts() {
            let raw = inst.id().raw();
            // The markers before it.
            while next_marker < all_markers.len() && all_markers[next_marker].before.raw() <= raw {
                let marker = all_markers[next_marker];
                next_marker += 1;
                let Some(root) = root_of(&aliases, marker.ptr) else { continue };
                let via = inst_of(marker.ptr).unwrap_or(NONE);
                uses.push(SlotUse {
                    slot: root.slot,
                    inst: raw,
                    block: id_b,
                    via,
                    kind: UseKind::Marker,
                });
                events.push(Event { pos: raw, track: tracks.rule4(root.slot), op: Op::Use });
                if marker.start && root.offset == Some(0) {
                    markers.insert(root.slot);
                    events.push(Event { pos: raw, track: tracks.rule4(root.slot), op: Op::Gen });
                }
            }
            if cut_at[id_b.index()] == raw {
                events.push(Event { pos: raw, track: NONE, op: Op::Cut });
            }
            let kind = inst.kind();
            if let InstKind::Phi(phi) = &kind {
                for (value, pred) in phi.incoming() {
                    match value.kind() {
                        ValueKind::Inst(def) => {
                            if let Some(track) = tracks.value(def.raw()) {
                                edge_uses.push(EdgeUse { pred, succ: id_b, track });
                            } else if !recomputed_value(body, def) {
                                // Defined in the predecessor itself: it can
                                // only cross on that very edge.
                                edge_uses.push(EdgeUse {
                                    pred,
                                    succ: id_b,
                                    track: def.raw() | 1 << 31,
                                });
                            }
                            if let Some(root) = aliases.get(def.raw()) {
                                escaped.insert(root.slot);
                                uses.push(SlotUse {
                                    slot: root.slot,
                                    inst: raw,
                                    block: id_b,
                                    via: def.raw(),
                                    kind: UseKind::Escape,
                                });
                            }
                        }
                        ValueKind::Arg(_) => {
                            edge_uses.push(EdgeUse { pred, succ: id_b, track: NONE });
                        }
                        ValueKind::Slot(slot) => {
                            escaped.insert(slot);
                            uses.push(SlotUse {
                                slot,
                                inst: raw,
                                block: id_b,
                                via: NONE,
                                kind: UseKind::Escape,
                            });
                        }
                        _ => {}
                    }
                }
            } else {
                // Each operand: a slot use, a value use, an argument use.
                let operands = inst.operands();
                for (position, &value) in operands.iter().enumerate() {
                    match value.kind() {
                        ValueKind::Arg(_) => arg_uses.push((raw, id_b)),
                        ValueKind::Inst(def) => {
                            if let Some(track) = tracks.value(def.raw()) {
                                events.push(Event { pos: raw, track, op: Op::Use });
                            }
                        }
                        _ => {}
                    }
                    let Some(root) = root_of(&aliases, value) else { continue };
                    let via = inst_of(value).unwrap_or(NONE);
                    let access = slot_access(
                        module,
                        body,
                        &kind,
                        position,
                        value,
                        root,
                        slot_size(root.slot),
                        inst.opcode(),
                    );
                    let r4 = tracks.rule4(root.slot);
                    let r3 = Tracks::rule3(root.slot);
                    let use_kind = match access {
                        Access::Derive => UseKind::Derive,
                        Access::Escape => {
                            escaped.insert(root.slot);
                            UseKind::Escape
                        }
                        Access::Promise | Access::Read | Access::WriteWhole | Access::WritePart => {
                            UseKind::Access
                        }
                    };
                    uses.push(SlotUse {
                        slot: root.slot,
                        inst: raw,
                        block: id_b,
                        via,
                        kind: use_kind,
                    });
                    match access {
                        Access::Derive | Access::Promise => {}
                        Access::Read => {
                            events.push(Event { pos: raw, track: r3, op: Op::Use });
                            events.push(Event { pos: raw, track: r4, op: Op::Use });
                        }
                        Access::WriteWhole => {
                            events.push(Event { pos: raw, track: r4, op: Op::Use });
                            events.push(Event { pos: raw, track: r3, op: Op::Def });
                        }
                        Access::WritePart => {
                            events.push(Event { pos: raw, track: r4, op: Op::Use });
                            events.push(Event { pos: raw, track: r3, op: Op::Gen });
                        }
                        Access::Escape => {
                            events.push(Event { pos: raw, track: r4, op: Op::Use });
                        }
                    }
                }
            }
            if let Some(track) = tracks.value(raw) {
                events.push(Event { pos: raw, track, op: Op::Def });
            }
        }
        block_events.push((id_b, first, events.len() as u32));
    }
    for &(block, first, end) in &block_events {
        event_start[block.index()] = first;
        let _ = end;
    }
    let mut event_end: Vec<u32> = vec![0; block_count];
    for &(block, _, end) in &block_events {
        event_end[block.index()] = end;
    }
    // An alias that crosses a resumption edge is an escape: found by the
    // dataflow below, applied after it.

    // Edge uses, grouped by predecessor.
    edge_uses.sort_by_key(|edge| edge.pred.index());
    let mut edge_start: Vec<u32> = vec![0; block_count + 1];
    for edge in &edge_uses {
        edge_start[edge.pred.index() + 1] += 1;
    }
    for at in 0..block_count {
        edge_start[at + 1] += edge_start[at];
    }

    // ---- the dataflow ----
    let hits = flow(
        &cfg,
        &coro.suspends,
        &tracks,
        &events,
        &event_start,
        &event_end,
        &edge_uses,
        &edge_start,
    );

    // ---- the resumed activation ----
    let resumed = reach(
        &cfg,
        &coro.suspends,
        coro.suspends.iter().flat_map(|s| [s.resume, s.destroy]),
        |block| cut_at[block.index()] == NONE,
        |edge| edge != Edge::RampOnly,
    );
    let after_resumption =
        |inst: u32, block: BlockId| resumed.contains(block) && inst < cut_at[block.index()];
    let edge_kind = |pred: BlockId, succ: BlockId| {
        let mut kind = Edge::Normal;
        cfg.edges(pred, &coro.suspends, |to, edge| {
            if to == succ && edge != Edge::Normal {
                kind = edge;
            }
        });
        kind
    };
    let edge_after_resumption = |pred: BlockId, succ: BlockId| match edge_kind(pred, succ) {
        Edge::Resume => true,
        Edge::RampOnly => false,
        _ => resumed.contains(pred) && cut_at[pred.index()] == NONE,
    };

    // Arguments.
    for &(inst, block) in &arg_uses {
        if after_resumption(inst, block) {
            return refuse(ARGUMENT_ACROSS_SUSPEND, Some(InstId::new(inst)));
        }
    }
    for edge in &edge_uses {
        if edge.track == NONE && edge_after_resumption(edge.pred, edge.succ) {
            let at = body.block(edge.pred).terminator().id();
            return refuse(ARGUMENT_ACROSS_SUSPEND, Some(at));
        }
    }

    // ---- the values ----
    let inst_count = body.inst_count();
    let mut heap_values: BitSet<InstId> = BitSet::new(inst_count);
    for (k, &raw) in tracks.values.iter().enumerate() {
        if hits.contains(InstId::new(2 * tracks.slots + k as u32)) {
            heap_values.insert(InstId::new(raw));
        }
    }
    for edge in &edge_uses {
        if edge.track != NONE && edge_kind(edge.pred, edge.succ) == Edge::Resume {
            let raw = if edge.track & 1 << 31 != 0 && edge.track != NONE {
                edge.track & !(1 << 31)
            } else {
                tracks.values[(edge.track - 2 * tracks.slots) as usize]
            };
            heap_values.insert(InstId::new(raw));
        }
    }
    for (&raw, root) in aliases.ids.iter().zip(&aliases.roots) {
        if let Some(root) = root
            && heap_values.contains(InstId::new(raw))
        {
            escaped.insert(root.slot);
        }
    }

    // ---- the slots ----
    let mut classes = Vec::with_capacity(slot_count);
    let mut heap_slots: BitSet<SlotId> = BitSet::new(slot_count);
    for (index, slot) in body.slots().iter().enumerate() {
        let id_s = SlotId::new(index as u32);
        let class = if promise == Some(id_s) {
            SlotClass::Promise
        } else if slot.outside_frame() {
            SlotClass::OutsideFrame
        } else if !escaped.contains(id_s) {
            SlotClass::Unescaped
        } else if markers.contains(id_s) {
            SlotClass::EscapedWithMarkers
        } else {
            SlotClass::EscapedWithoutMarkers
        };
        let heap = match class {
            SlotClass::Promise | SlotClass::OutsideFrame => false,
            SlotClass::Unescaped => hits.contains(InstId::new(Tracks::rule3(id_s))),
            SlotClass::EscapedWithMarkers => hits.contains(InstId::new(tracks.rule4(id_s))),
            SlotClass::EscapedWithoutMarkers => true,
        };
        if heap {
            heap_slots.insert(id_s);
        }
        classes.push(class);
    }
    for u in &uses {
        if classes[u.slot.index()] == SlotClass::OutsideFrame
            && u.kind != UseKind::Marker
            && after_resumption(u.inst, u.block)
        {
            return Err(Refusal {
                construct: OUTSIDE_FRAME_ACROSS_SUSPEND,
                inst: Some(InstId::new(u.inst)),
                slot: Some(u.slot),
            });
        }
    }

    // ---- before coro.begin ----
    let mut copies: BitSet<SlotId> = BitSet::new(slot_count);
    for u in &uses {
        if !heap_slots.contains(u.slot) || u.kind == UseKind::Marker {
            continue;
        }
        if pre_begin(u.inst, u.block) {
            copies.insert(u.slot);
        } else if u.via != NONE && pre_begin(u.via, cfg.block_of(u.via)) {
            return Err(Refusal {
                construct: FRAME_BEFORE_BEGIN,
                inst: Some(InstId::new(u.inst)),
                slot: Some(u.slot),
            });
        }
    }
    let begin_copies: Vec<SlotId> = copies.iter().collect();
    let begin_value_copies: Vec<InstId> = heap_values
        .iter()
        .filter(|&value| pre_begin(value.raw(), cfg.block_of(value.raw())))
        .collect();

    // ---- the safety conditions ----
    let mut free_blocks = BitSet::new(block_count);
    let mut free_tails: Vec<u32> = vec![NONE; block_count];
    for call in body.calls() {
        let frees_frame = call.intrinsic().is_none()
            && call
                .args()
                .iter()
                .any(|&arg| inst_of(arg).is_some_and(|def| coro.frees.binary_search(&def).is_ok()));
        if frees_frame {
            let at = call.inst().raw();
            let block = cfg.block_of(at);
            free_tails[block.index()] = free_tails[block.index()].min(at);
            let after = reach(
                &cfg,
                &coro.suspends,
                successors(&cfg, &coro.suspends, block),
                |_| true,
                |_| true,
            );
            for block in after.iter() {
                free_blocks.insert(block);
            }
        }
    }
    let after_free = |inst: u32, block: BlockId| {
        free_blocks.contains(block)
            || (free_tails[block.index()] != NONE && inst > free_tails[block.index()])
    };
    let where_forbidden = |inst: u32, block: BlockId| {
        if in_region(inst, block) {
            Some(Region::Suspended)
        } else if after_free(inst, block) {
            Some(Region::AfterFree)
        } else {
            None
        }
    };
    let mut violations = Vec::new();
    for u in &uses {
        if !heap_slots.contains(u.slot) || u.kind == UseKind::Marker {
            continue;
        }
        if let Some(region) = where_forbidden(u.inst, u.block) {
            violations.push(Violation {
                slot: Some(u.slot),
                value: None,
                at: InstId::new(u.inst),
                region,
            });
        }
    }
    for block in body.blocks() {
        let id_b = block.id();
        let range = event_start[id_b.index()] as usize..event_end[id_b.index()] as usize;
        for event in &events[range] {
            if event.track == NONE || event.track < 2 * tracks.slots {
                continue;
            }
            let raw = tracks.values[(event.track - 2 * tracks.slots) as usize];
            if heap_values.contains(InstId::new(raw))
                && let Some(region) = where_forbidden(event.pos, id_b)
            {
                violations.push(Violation {
                    slot: None,
                    value: Some(InstId::new(raw)),
                    at: InstId::new(event.pos),
                    region,
                });
            }
        }
    }
    for edge in &edge_uses {
        if edge.track == NONE {
            continue;
        }
        let raw = if edge.track & 1 << 31 != 0 {
            edge.track & !(1 << 31)
        } else {
            tracks.values[(edge.track - 2 * tracks.slots) as usize]
        };
        if !heap_values.contains(InstId::new(raw)) {
            continue;
        }
        let at = body.block(edge.pred).terminator().id();
        let region = if edge_kind(edge.pred, edge.succ) == Edge::Suspended {
            Some(Region::Suspended)
        } else {
            where_forbidden(at.raw(), edge.pred)
        };
        if let Some(region) = region {
            violations.push(Violation { slot: None, value: Some(InstId::new(raw)), at, region });
        }
    }

    // ---- the frame ----
    let mut frame_align: u64 = 16;
    let promise_offset = match promise {
        Some(slot) => {
            let align = body.slots()[slot.index()].align();
            frame_align = frame_align.max(align);
            16u64.next_multiple_of(align)
        }
        None => 16,
    };
    for slot in heap_slots.iter() {
        frame_align = frame_align.max(body.slots()[slot.index()].align());
    }
    for value in heap_values.iter() {
        frame_align = frame_align.max(types.align_of(body.inst(value).ty()));
    }

    // ---- the suspends, numbered ----
    let mut order: Vec<&RawSuspend> = coro.suspends.iter().collect();
    order.sort_by_key(|s| (s.is_final, s.inst));
    let suspends: Vec<Suspend> = order
        .iter()
        .enumerate()
        .map(|(index, s)| Suspend {
            save: InstId::new(s.save),
            wrapper: (s.wrapper != NONE).then(|| InstId::new(s.wrapper)),
            suspend: InstId::new(s.inst),
            index: index as u32,
            is_final: s.is_final,
        })
        .collect();
    let final_index = suspends.iter().find(|s| s.is_final).map(|s| s.index);

    let plan = CoroPlan {
        begin: InstId::new(begin),
        promise,
        promise_offset: u32::try_from(promise_offset).unwrap_or(u32::MAX),
        frame_align: u32::try_from(frame_align).unwrap_or(u32::MAX),
        heap_slots,
        heap_values,
        begin_copies,
        begin_value_copies,
        suspends,
        suspended_region: region,
        has_unwind_end,
        final_index,
    };
    check_invariants(&plan, &classes);
    Ok(Some(Analysis { plan, classes, violations, calls_align: coro.calls_align }))
}

/// The properties every plan has, by construction (`spec/xform.md` §3.6).
fn check_invariants(plan: &CoroPlan, classes: &[SlotClass]) {
    for slot in plan.heap_slots.iter() {
        let class = classes[slot.index()];
        assert!(
            !matches!(class, SlotClass::Promise | SlotClass::OutsideFrame),
            "the promise or an outside-frame slot is in the heap"
        );
    }
    for slot in &plan.begin_copies {
        assert!(plan.heap_slots.contains(*slot), "a begin copy of a stack slot");
    }
    for value in &plan.begin_value_copies {
        assert!(plan.heap_values.contains(*value), "a begin copy of a stack value");
    }
    assert!(plan.frame_align >= 16 && plan.frame_align.is_power_of_two(), "frame alignment");
    assert!(plan.promise_offset >= 16, "the promise overlaps the header");
    for (index, s) in plan.suspends.iter().enumerate() {
        assert_eq!(s.index as usize, index, "suspends out of index order");
        assert!(!s.is_final || index + 1 == plan.suspends.len(), "the final suspend is not last");
    }
}

/// Whether `def` is a result the code generator recomputes.
fn recomputed_value(body: &Body, def: InstId) -> bool {
    def.index() >= body.inst_count()
        || recomputed(body.inst(def).call().and_then(|call| call.intrinsic()))
}

/// What one operand does to the slot its pointer refers to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Access {
    Read,
    WriteWhole,
    WritePart,
    Escape,
    Derive,
    /// The promise operand of `coro.id`.
    Promise,
}

#[allow(clippy::too_many_arguments, reason = "the operand and what it refers to")]
fn slot_access(
    module: &Module,
    body: &Body,
    kind: &InstKind<'_>,
    position: usize,
    value: Value,
    root: Root,
    slot_size: u64,
    opcode: Opcode,
) -> Access {
    let types = &module.types;
    let whole = |offset: Option<i64>, size: Option<u64>| {
        if offset == Some(0) && size.is_some_and(|size| size >= slot_size) {
            Access::WriteWhole
        } else {
            Access::WritePart
        }
    };
    match kind {
        InstKind::Load { ptr, .. } if *ptr == value => Access::Read,
        InstKind::Store { value: stored, ptr, .. } if *ptr == value && *stored != value => {
            let size = types.size_of(body.value_type(module, *stored));
            whole(root.offset, Some(size))
        }
        InstKind::PtrAdd(add) if add.base == value && position == 0 => Access::Derive,
        // Casts that keep the address derive an alias; the others
        // (`ptrtoint`) publish it.
        InstKind::Cast { .. } if matches!(opcode, Opcode::BitCast | Opcode::AddrSpaceCast) => {
            Access::Derive
        }
        InstKind::Call(call) => {
            // `operands()` lists the callee first.
            let Some(arg) = position.checked_sub(1) else { return Access::Escape };
            let args = call.args();
            if arg >= args.len() {
                return Access::Escape;
            }
            match call.intrinsic() {
                Some(Intrinsic::Memcpy | Intrinsic::MemcpyInline | Intrinsic::Memmove)
                    if arg < 2 && args[0] != args[1] =>
                {
                    if arg == 1 {
                        Access::Read
                    } else {
                        whole(root.offset, int_arg(module, args.get(2)))
                    }
                }
                Some(Intrinsic::Memset | Intrinsic::MemsetInline) if arg == 0 => {
                    whole(root.offset, int_arg(module, args.get(2)))
                }
                Some(Intrinsic::CoroId) if arg == 1 => Access::Promise,
                _ => Access::Escape,
            }
        }
        _ => Access::Escape,
    }
}

/// Marks the conditional branch on `cond` (an `is_in_ramp`, or LLVM 19's
/// unwinding `i1 coro.end`) as having a ramp-only side: the true side of
/// `is_in_ramp` (`ramp_if_true`), the false side of `coro.end`.
fn ramp_branch(cfg: &mut Cfg<'_>, cond: u32, ramp_if_true: bool) {
    let body = cfg.body;
    let block = cfg.block_of(cond);
    let term = body.block(block).terminator();
    if let InstKind::CondBr { cond: tested, if_true, if_false } = term.kind()
        && inst_of(tested) == Some(cond)
        && if_true != if_false
        && matches!(cfg.term[block.index()], Term::Plain)
    {
        let (ramp, other) = if ramp_if_true { (if_true, if_false) } else { (if_false, if_true) };
        cfg.term[block.index()] = Term::Ramp { ramp, other };
    }
}

/// From the wrapper (or save) `from`, the single path to a suspend: the
/// rest of its block, an `invoke`'s normal edge, an unconditional branch,
/// the true side of a `.bool` wrapper's result. Adds the blocks after
/// `from`'s to `region`; the suspend's index, or `None`.
fn walk_to_suspend(
    cfg: &Cfg<'_>,
    suspends: &[RawSuspend],
    from: u32,
    is_bool: bool,
    region: &mut BitSet<BlockId>,
) -> Option<usize> {
    let body = cfg.body;
    let mut block = cfg.block_of(from);
    let mut at = from;
    let mut path: Vec<BlockId> = Vec::new();
    for _ in 0..=body.block_count() {
        let range = body.block(block).inst_range();
        for raw in at..range.end {
            if let Some(n) = suspends.iter().position(|s| s.inst == raw) {
                for &b in &path {
                    region.insert(b);
                }
                return Some(n);
            }
        }
        let term = body.block(block).terminator();
        let next = match term.kind() {
            InstKind::Br { target } => target,
            InstKind::CondBr { cond, if_true, .. } if is_bool && inst_of(cond) == Some(from) => {
                if_true
            }
            InstKind::Call(call) if term.id().raw() == from => call.normal_dest()?,
            _ => return None,
        };
        path.push(next);
        block = next;
        at = range_start(body, next);
    }
    None
}

fn range_start(body: &Body, block: BlockId) -> u32 {
    body.block(block).inst_range().start
}

fn successors(cfg: &Cfg<'_>, suspends: &[RawSuspend], block: BlockId) -> Vec<BlockId> {
    let mut out = Vec::new();
    cfg.edges(block, suspends, |succ, _| out.push(succ));
    out
}

fn reverse_post_order(cfg: &Cfg<'_>, suspends: &[RawSuspend]) -> Vec<BlockId> {
    let body = cfg.body;
    let mut seen: BitSet<BlockId> = BitSet::new(body.block_count());
    let mut post = Vec::with_capacity(body.block_count());
    // (block, its next edge to visit)
    let mut stack: Vec<(BlockId, usize)> = Vec::new();
    let entry = body.entry();
    seen.insert(entry);
    stack.push((entry, 0));
    while let Some((block, next)) = stack.last_mut() {
        if let Some(succ) = cfg.edge_at(*block, *next, suspends) {
            *next += 1;
            if seen.insert(succ) {
                stack.push((succ, 0));
            }
        } else {
            post.push(*block);
            stack.pop();
        }
    }
    post.reverse();
    post
}

/// The forward dataflow of `spec/xform.md` §3.4: two bits per track,
/// "stored" (A) and "a resumption edge passed since" (C), joined by
/// union. Returns the tracks with a use that saw both.
#[allow(clippy::too_many_arguments, reason = "the tables the analysis built")]
fn flow(
    cfg: &Cfg<'_>,
    suspends: &[RawSuspend],
    tracks: &Tracks,
    events: &[Event],
    event_start: &[u32],
    event_end: &[u32],
    edge_uses: &[EdgeUse],
    edge_start: &[u32],
) -> BitSet<InstId> {
    let n = tracks.len();
    let words = n.div_ceil(64).max(1);
    let blocks = cfg.body.block_count();
    let mut in_a = vec![0u64; blocks * words];
    let mut in_c = vec![0u64; blocks * words];
    let mut a = vec![0u64; words];
    let mut c = vec![0u64; words];
    let mut hits: BitSet<InstId> = BitSet::new(n);
    let bit = |set: &[u64], track: u32| set[track as usize / 64] >> (track % 64) & 1 != 0;
    loop {
        let mut changed = false;
        for &block in &cfg.rpo {
            let at = block.index() * words;
            a.copy_from_slice(&in_a[at..at + words]);
            c.copy_from_slice(&in_c[at..at + words]);
            let range = event_start[block.index()] as usize..event_end[block.index()] as usize;
            for event in &events[range] {
                let (word, mask) = (event.track as usize / 64, 1u64 << (event.track % 64));
                match event.op {
                    Op::Def => {
                        a[word] |= mask;
                        c[word] &= !mask;
                    }
                    Op::Gen => a[word] |= mask,
                    Op::Use => {
                        if bit(&c, event.track) {
                            hits.insert(InstId::new(event.track));
                        }
                    }
                    Op::Cut => c.fill(0),
                }
            }
            let range = edge_start[block.index()] as usize..edge_start[block.index() + 1] as usize;
            for edge in &edge_uses[range] {
                if edge.track < n as u32 && bit(&c, edge.track) {
                    hits.insert(InstId::new(edge.track));
                }
            }
            cfg.edges(block, suspends, |succ, edge| {
                let to = succ.index() * words;
                for w in 0..words {
                    let new_c = match edge {
                        Edge::Resume => c[w] | a[w],
                        Edge::RampOnly => 0,
                        Edge::Normal | Edge::Suspended => c[w],
                    };
                    let (old_a, old_c) = (in_a[to + w], in_c[to + w]);
                    in_a[to + w] |= a[w];
                    in_c[to + w] |= new_c;
                    changed |= in_a[to + w] != old_a || in_c[to + w] != old_c;
                }
            });
        }
        if !changed {
            break;
        }
    }
    hits
}
