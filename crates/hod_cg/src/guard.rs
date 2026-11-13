//! Stack protectors (`spec/codegen.md` §5.6.24, WP-183): which functions
//! get a guard, which body slots move into the **guarded block**, and the
//! block's layout. The target emits the code: the entry stores
//! ([`Target::enter_guard`](crate::Target::enter_guard)), the block's
//! place at the top of the frame and the checks at every exit, from
//! `end_function`.
//!
//! A function with `ssp`, `sspstrong` or `sspreq` is protected when it has
//! `sspreq`, a body slot, or an `alloca` instruction. That is a superset of
//! LLVM's choice (`StackProtector::requiresStackProtector`), which needs an
//! array, an address-taken local or an `alloca` for the first two: hod
//! keeps no slot types, so it cannot tell an array from a scalar, and
//! never protecting fewer functions than `llc` is what matters.
//!
//! The slots that can be overflowed are those whose address is used other
//! than as the pointer of a `load` or `store` that stays inside the slot
//! (an escaping slot: a `getelementptr`, a call argument, a stored
//! pointer, ...). They go into the guarded block in body order, each
//! addressed through a pointer slot ([`SlotPlace::Indirect`]); the guard
//! word ends the block. Every other slot cannot be written out of bounds
//! and stays where §5.1 puts it, below the block.

use hod_ir::{Body, FnAttrs, InstKind, Module, Opcode};

use crate::frame::FrameRef;

/// A protected presplit coroutine: its resume stubs enter the body past
/// the entry code that sets the guard (§7.6).
pub const SSP_COROUTINE: &str = "ssp:coroutine";

/// A target without stack-protector code (`Target::enter_guard`'s
/// default).
pub const SSP_TARGET: &str = "ssp:target";

/// The bytes of the guard word.
pub const GUARD_BYTES: u32 = 8;

/// The guarded block of a protected function (§5.6.24). Reset, not freed,
/// between functions.
#[derive(Clone, Debug, Default)]
pub struct GuardPlan {
    on: bool,
    low: bool,
    block: u32,
    guard: u32,
    /// By slot index: the slot's offset in the block + 1; 0 for a slot
    /// that is not in it.
    in_block: Vec<u32>,
    /// The pointer slot of each slot in the block and the slot's offset
    /// in the block, in body order; filled by the slot layout.
    pointers: Vec<(FrameRef, u32)>,
}

impl GuardPlan {
    /// Whether the function has a stack guard.
    #[inline]
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Whether the function also needs the **low guard**, between its
    /// dynamic area and the rest of its frame: it has an `alloca`
    /// instruction or an over-aligned slot (§5.1, §5.2).
    #[inline]
    pub fn low(&self) -> bool {
        self.low
    }

    /// The block's size in bytes: a multiple of 16, at most 15 bytes of
    /// padding after the guard word.
    #[inline]
    pub fn block_size(&self) -> u32 {
        self.block
    }

    /// The guard word's offset in the block: right after its last slot,
    /// not necessarily aligned; at most padding follows it.
    #[inline]
    pub fn guard_offset(&self) -> u32 {
        self.guard
    }

    /// The pointer slot of each slot in the block, with the slot's offset
    /// in the block. Each pointer slot is written by the entry code.
    #[inline]
    pub fn pointers(&self) -> &[(FrameRef, u32)] {
        &self.pointers
    }

    pub(crate) fn reset(&mut self) {
        self.on = false;
        self.low = false;
        self.block = 0;
        self.guard = 0;
        self.in_block.clear();
        self.pointers.clear();
    }

    /// The offset in the block of body slot `index`, if it is in it.
    pub(crate) fn slot_offset(&self, index: usize) -> Option<u32> {
        self.in_block.get(index).and_then(|&at| at.checked_sub(1))
    }

    pub(crate) fn push_pointer(&mut self, pointer: FrameRef, offset: u32) {
        self.pointers.push((pointer, offset));
    }

    /// Decides whether the function `body` (function flags `flags`) is
    /// protected and lays out its block. `stack_align` is the target's:
    /// a slot aligned beyond it is dynamic (§5.1) and is not in the
    /// block.
    ///
    /// # Errors
    /// [`SSP_COROUTINE`] for a protected presplit coroutine.
    pub(crate) fn plan(
        &mut self,
        module: &Module,
        body: &Body,
        flags: u32,
        stack_align: u32,
    ) -> Result<(), &'static str> {
        self.reset();
        let any = FnAttrs::SSP | FnAttrs::SSP_STRONG | FnAttrs::SSP_REQ;
        if flags & any == 0 {
            return Ok(());
        }
        let slots = body.slots();
        let dynamic = body.insts().any(|inst| inst.opcode() == Opcode::Alloca);
        let over_aligned = slots.iter().any(|slot| slot.align() > u64::from(stack_align));
        if flags & FnAttrs::SSP_REQ == 0 && slots.is_empty() && !dynamic {
            return Ok(());
        }
        if flags & FnAttrs::PRESPLIT_COROUTINE != 0 {
            return Err(SSP_COROUTINE);
        }
        self.on = true;
        self.low = dynamic || over_aligned;
        self.in_block.resize(slots.len(), 0);
        // 1: the slot escapes; layout below turns it into an offset.
        for inst in body.insts() {
            let plain = match inst.kind() {
                InstKind::Load { ptr, .. } => Some((ptr, body.value_type(module, inst.result()))),
                InstKind::Store { value, ptr, .. } => {
                    if let Some(slot) = value.as_slot() {
                        self.in_block[slot.index()] = 1;
                    }
                    Some((ptr, body.value_type(module, value)))
                }
                _ => None,
            };
            if let Some((ptr, ty)) = plain {
                if let Some(slot) = ptr.as_slot() {
                    let size = module.types.size_of(ty);
                    let fits =
                        slots.get(slot.index()).is_some_and(|s| size != 0 && size <= s.size());
                    if !fits {
                        self.in_block[slot.index()] = 1;
                    }
                }
                continue;
            }
            for value in inst.operands() {
                if let Some(slot) = value.as_slot()
                    && let Some(mark) = self.in_block.get_mut(slot.index())
                {
                    *mark = 1;
                }
            }
        }
        let mut end = 0u64;
        for (mark, slot) in self.in_block.iter_mut().zip(slots) {
            if *mark == 0 || slot.align() > u64::from(stack_align) {
                *mark = 0;
                continue;
            }
            let at = end.next_multiple_of(slot.align().max(1));
            end = at + slot.size();
            *mark = u32::try_from(at + 1).map_err(|_| crate::error::LIMIT_FRAME_SIZE)?;
        }
        // The guard right after the last slot, unaligned if need be, so
        // that an overflow of one byte reaches it; above it only padding
        // to the block's end, then what the frame saves.
        let guard = end;
        let block = (guard + u64::from(GUARD_BYTES)).next_multiple_of(16);
        if block > u64::from(crate::frame::MAX_FRAME) {
            return Err(crate::error::LIMIT_FRAME_SIZE);
        }
        self.block = block as u32;
        self.guard = guard as u32;
        Ok(())
    }
}
