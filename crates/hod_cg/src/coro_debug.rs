//! The variable locations of a coroutine body (`spec/codegen.md` §7.6
//! "Debug information", `spec/dwarf.md` §6.16, WP-108).
//!
//! A coroutine is compiled once, and its activations (the ramp, and each
//! one a stub entered) share one machine frame layout, so a variable on
//! the machine stack keeps its frame-base offset. What lives in the heap
//! frame is described through the **frame word** (`CoroSlots::frame`),
//! which the prologue sets to 0 and `coro.begin` (in the ramp) or the
//! stub (in a resumed activation) sets to the heap frame's address:
//!
//! | the declared address | its place |
//! |---|---|
//! | a heap slot, or a value homed in the heap | `CoroAt::Heap` |
//! | a slot or value copied at `coro.begin` (its shadow before, its heap place after) | `CoroAt::Copied` |
//! | one variable declared on a machine-stack slot and on a heap slot (clang's parameter and its frame copy) | one row, `CoroAt::Copied` |
//! | a parameter on a machine-stack slot, and its frame copy declared as an artificial variable of the same name and scope in the heap (kiln) | both `CoroAt::Copied` |
//! | a parameter on a machine-stack slot only (`!coro.outside.frame`) | `CoroAt::Ramp`: written by the ramp only |
//!
//! Every other row keeps its place: a local on the machine stack is valid
//! in the activation that wrote it, as any `-O0` local is.

use hod_dwarf::{CoroAt, CoroPlace};
use hod_ir::{DiFlags, DiNode};

use crate::coro_body::CoroSlots;
use crate::cx::FnCx;
use crate::frame::FrameRef;
use crate::record::VarPlace;
use crate::target::Target;

/// What `coro_vars` needs of a coroutine body ([`FnCx::coro_debug_view`]).
pub(crate) struct View {
    /// The mode and frame words.
    pub(crate) words: CoroSlots,
    /// The `coro.begin` copies of slots: (shadow, heap place).
    pub(crate) slot_copies: Vec<(FrameRef, FrameRef)>,
    /// The `coro.begin` copies of values: (shadow, heap home).
    pub(crate) value_copies: Vec<(FrameRef, FrameRef)>,
}

impl<T: Target> FnCx<'_, T> {
    /// From `finish_vars`, once the frame is final and before the frame
    /// places become offsets: the rows of a coroutine body's variables
    /// from `start` on, rewritten as the module doc says. Nothing outside
    /// a coroutine.
    pub(crate) fn coro_vars(&mut self, start: usize) {
        let Some(view) = self.coro_debug_view() else { return };
        let (t, desc) = (self.target, self.frame_desc);
        let base = |place: FrameRef| t.frame_base_offset(&desc, place);
        let (Some(word), Some(mode)) = (base(view.words.frame), base(view.words.mode)) else {
            return; // the generic rows refuse what the frame base does not reach
        };
        let heap_of = |copies: &[(FrameRef, FrameRef)], shadow: FrameRef| {
            copies.iter().find(|&&(s, _)| s == shadow).map(|&(_, heap)| heap)
        };
        let shadow_of = |copies: &[(FrameRef, FrameRef)], heap: FrameRef| {
            copies.iter().find(|&&(_, h)| h == heap).map(|&(shadow, _)| shadow)
        };
        let coro = |at, indirect| VarPlace::Coro(CoroPlace { word, at, indirect });
        let Some(rows) = self.out.vars.get_mut(start..) else { return };
        // The heap, and the copies.
        for row in rows.iter_mut() {
            let (place, indirect) = match row.place {
                VarPlace::Slot(place) => (place, false),
                VarPlace::Indirect(place) => (place, true),
                _ => continue,
            };
            let copies = if indirect { &view.value_copies } else { &view.slot_copies };
            let at = match place {
                FrameRef::Coro(heap) => match shadow_of(copies, place).and_then(base) {
                    Some(stack) => CoroAt::Copied { heap, stack },
                    None => CoroAt::Heap(heap),
                },
                shadow => match (heap_of(copies, shadow), base(shadow)) {
                    (Some(FrameRef::Coro(heap)), Some(stack)) => CoroAt::Copied { heap, stack },
                    _ => continue,
                },
            };
            row.place = coro(at, indirect);
        }
        // One variable declared on a machine-stack slot and on a heap
        // slot: before the frame exists the first, after it the second.
        let mut merged = Vec::new();
        let mut i = 0;
        while i < rows.len() {
            let VarPlace::Slot(stack @ FrameRef::Local(_)) = rows[i].place else {
                i += 1;
                continue;
            };
            let var = rows[i].var;
            let twin = rows.iter().enumerate().position(|(k, r)| {
                r.var == var
                    && !merged.contains(&(start + k))
                    && matches!(
                        r.place,
                        VarPlace::Coro(CoroPlace { at: CoroAt::Heap(_), indirect: false, .. })
                    )
            });
            if let (Some(j), Some(stack)) = (twin, base(stack))
                && let VarPlace::Coro(CoroPlace { at: CoroAt::Heap(heap), .. }) = rows[j].place
            {
                rows[i].place = coro(CoroAt::Copied { heap, stack }, false);
                merged.push(start + j);
            }
            i += 1;
        }
        // A parameter on the machine stack only: the ramp's; or, when the
        // front end declares its frame copy as an artificial variable of
        // the same name and scope in the heap (kiln), both are that
        // parameter, on the stack before `coro.begin` and in the heap after.
        let debug = &self.module.debug;
        let var = |id| match debug.node(id) {
            DiNode::LocalVariable(v) => Some(v),
            _ => None,
        };
        for k in 0..rows.len() {
            if merged.contains(&(start + k)) {
                continue;
            }
            let VarPlace::Slot(stack @ FrameRef::Local(_)) = rows[k].place else { continue };
            let (Some(v), Some(stack)) = (var(rows[k].var), base(stack)) else { continue };
            if v.arg == 0 {
                continue;
            }
            let copy = rows.iter().position(|r| {
                let heap = matches!(
                    r.place,
                    VarPlace::Coro(CoroPlace { at: CoroAt::Heap(_), indirect: false, .. })
                );
                heap && var(r.var).is_some_and(|c| {
                    c.arg == 0
                        && c.flags.contains(DiFlags::ARTIFICIAL)
                        && c.name == v.name
                        && c.scope == v.scope
                })
            });
            match copy.map(|j| (j, rows[j].place)) {
                Some((j, VarPlace::Coro(CoroPlace { at: CoroAt::Heap(heap), .. }))) => {
                    let place = coro(CoroAt::Copied { heap, stack }, false);
                    rows[k].place = place;
                    rows[j].place = place;
                }
                _ => rows[k].place = coro(CoroAt::Ramp { stack, mode }, false),
            }
        }
        merged.sort_unstable();
        for at in merged.into_iter().rev() {
            self.out.vars.remove(at);
        }
    }
}
