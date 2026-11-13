//! Coroutines at the facade (WP-106, WP-109; `spec/codegen.md` §7.6,
//! `spec/cli.md` "Pre-opt modules"): a body with `presplitcoroutine` that
//! `classify` accepted is accepted when the coroutine plan accepts it and
//! code generation will compile it, and refused by name otherwise, BEFORE
//! code generation. With debug information, its variables are described
//! on the frame word (WP-108, `spec/dwarf.md` §6.16).
//!
//! A front end hands hod a pre-opt module in place of `opt` + `llc` and
//! runs those itself when hod declines (status 2 in strict mode), so a
//! decline must cost next to nothing: every coroutine refusal is decided
//! here, by the module level (a matrix without `presplitcoroutine`, or a
//! target that compiles no coroutine body: before any body is read) or by the body as it arrives (the plan's
//! `coro:*` names and the code generator's own, checked by the same
//! rules), never by code generation half-way through the object. Code
//! generation keeps its checks as the backstop; `tests/coro_preopt.rs`
//! holds the two to the same names. The one exception is a handle
//! operation's: `a64:coro-promise` / `x64:coro-promise` (a
//! `llvm.coro.promise` alignment the target does not lower, in any
//! function) are left to code generation.

use hod_cg::Target;
use hod_ir::{
    Arch, BlockId, Body, CallConv, FnAttrs, FnAttrsId, Inst, Intrinsic, Module, SlotId, Support,
    SymbolKind, ValueKind,
};
use hod_xform::coro::CoroPlan;

/// The matrix row of the coroutine attribute (`docs/status/support.md`).
const PRESPLIT_ROW: &str = "attr:presplitcoroutine";

/// What the module level says about coroutines, found once by
/// [`module_refusal`].
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CoroModule {
    /// The module declares a `llvm.coro.*` function or has an attribute
    /// set with `presplitcoroutine`: only then are its bodies checked.
    any: bool,
}

/// Whether code generation for `arch` compiles coroutine bodies
/// (`hod_cg::Target::CORO_BODIES`).
fn bodies_compiled(arch: Arch) -> bool {
    match arch {
        Arch::Aarch64 => hod_a64::A64::CORO_BODIES,
        Arch::X86_64 => hod_x64::X64::CORO_BODIES,
    }
}

/// The stack alignment of `arch`'s code generator.
fn stack_align(arch: Arch) -> u64 {
    u64::from(match arch {
        Arch::Aarch64 => hod_a64::A64::STACK_ALIGN,
        Arch::X86_64 => hod_x64::X64::STACK_ALIGN,
    })
}

/// Whether any attribute set of `module` holds `presplitcoroutine`.
fn has_presplit(module: &Module) -> bool {
    (0..module.fn_attrs_count())
        .any(|index| module.fn_attrs(FnAttrsId::new(index as u32)).has(FnAttrs::PRESPLIT_COROUTINE))
}

/// The module level: what it says about coroutines, and the name the
/// module is declined by before any body is read, if any. A module with
/// a `presplitcoroutine` attribute set (a coroutine body as a front end
/// writes it before `coro-split`) is declined as `attr:presplitcoroutine`
/// when `support` does not accept the attribute on its target, and as
/// `coro:target` when the target's code generator compiles no coroutine
/// body (no shipped target today: the backstop for a new one).
pub(crate) fn module_refusal(
    module: &Module,
    support: &Support,
) -> (CoroModule, Option<&'static str>) {
    let presplit = has_presplit(module);
    let any = presplit
        || module.symbols().any(|symbol| {
            matches!(symbol.kind(), SymbolKind::Function { has_body: false, .. })
                && symbol.name().starts_with(b"llvm.coro.")
        });
    let refusal = if !presplit {
        None
    } else if support.supports(PRESPLIT_ROW) != Some(true) {
        Some(PRESPLIT_ROW)
    } else if !bodies_compiled(module.target().arch()) {
        Some(hod_cg::CORO_TARGET)
    } else {
        None
    };
    (CoroModule { any }, refusal)
}

/// The name `body` is refused by, if it is a coroutine hod does not
/// compile or a function that calls a coroutine body's intrinsic without
/// being one: the plan's `coro:*` refusals, then the code generator's
/// (`hod_cg::coro_body`, `hod_a64::coro_body`), decided here by the same
/// rules.
pub(crate) fn refusal(module: &Module, body: &Body, coro: CoroModule) -> Option<&'static str> {
    if !coro.any {
        return None;
    }
    match hod_xform::coro::plan(module, body) {
        Err(refusal) => Some(refusal.construct),
        Ok(None) => body.insts().any(is_body_intrinsic).then_some(hod_cg::OUTSIDE_COROUTINE),
        Ok(Some(plan)) => generator_refusal(module, body, &plan),
    }
}

/// What code generation would refuse in the coroutine `body` with `plan`.
fn generator_refusal(module: &Module, body: &Body, plan: &CoroPlan) -> Option<&'static str> {
    let arch = module.target().arch();
    if !bodies_compiled(arch) {
        return Some(hod_cg::CORO_TARGET);
    }
    // `hod_a64::coro_body::save`: x19 is the frame base, and these
    // conventions make it a register the caller does not save.
    if arch == Arch::Aarch64 {
        let cc = match module.symbol(body.symbol()).kind() {
            SymbolKind::Function { cc, .. } => cc,
            _ => CallConv::C,
        };
        if cc == CallConv::PRESERVE_MOST || cc == CallConv::PRESERVE_ALL {
            return Some(hod_a64::coro_body::PRESERVE_CC);
        }
    }
    // `FnCx::coro_slot_place`: a slot left on the machine stack, aligned
    // above the stack alignment.
    let align = stack_align(arch);
    let overaligned = body.slots().iter().enumerate().any(|(index, slot)| {
        let id = SlotId::new(index as u32);
        plan.promise != Some(id) && !plan.heap_slots.contains(id) && slot.align() > align
    });
    if overaligned {
        return Some(hod_cg::OVERALIGNED_STACK);
    }
    shares_begin_block(body, plan).then_some(hod_cg::BEGIN_SHARED_BLOCK)
}

/// `hod_cg::coro_body::pre_begin`'s refusal: a block that runs both
/// before `coro.begin` (reachable from the entry without passing
/// `coro.begin`'s block) and after it (reachable from that block's
/// successors) and names a slot or a value copied at `coro.begin`.
fn shares_begin_block(body: &Body, plan: &CoroPlan) -> bool {
    let begin_raw = plan.begin.raw();
    let Some(begin_block) = body.blocks().find(|b| b.inst_range().contains(&begin_raw)) else {
        return false;
    };
    let begin_block = begin_block.id();
    let blocks = body.block_count();
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
            return true;
        }
        work.extend(body.block(block).terminator().successors());
    }
    false
}

/// Whether `block` names a slot or a value copied at `coro.begin`.
fn mentions_copy(body: &Body, block: BlockId, plan: &CoroPlan) -> bool {
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

/// Whether `inst` calls an intrinsic only a coroutine body may call
/// (`hod_cg::coro_body`'s list).
fn is_body_intrinsic(inst: Inst<'_>) -> bool {
    use Intrinsic as I;
    inst.call().and_then(|call| call.intrinsic()).is_some_and(|which| {
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
    })
}
