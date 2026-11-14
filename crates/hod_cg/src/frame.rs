//! The frame allocator: `spec/codegen.md` §5.1 to §5.3.
//!
//! One **local area** per function, laid out by a bump allocator; its
//! final size is known when the function has been emitted. The target
//! turns a [`FrameRef`] into an addressing mode (§5.4).
//!
//! WP-198 (§5.1 "Value slots"): the bytes a bump skips to align a slot
//! are kept as **holes**, which value slots (homes, incoming slots,
//! spill slots) fill before the area grows; and **block slots**, the
//! homes of values whose every use is in their own block, are handed
//! out again in every block.

use crate::error::{CgError, CgResult, LIMIT_FRAME_SIZE, internal};
use crate::loc::Class;

/// The largest local area the allocator lays out. A target's prologue may
/// express less; it then reports the same limit from `end_function`.
pub const MAX_FRAME: u32 = 1 << 30;

/// A place in the frame, or next to it (§5.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FrameRef {
    /// Byte `offset` of the function's local area.
    Local(u32),
    /// Byte `offset` of the caller's stack-argument area: where this
    /// function's own stack-passed arguments are.
    Incoming(u32),
    /// Byte `offset` above the stack pointer, between the point where a
    /// call lowered it for stack arguments and the call instruction
    /// (§5.3). Only the call sequence produces these.
    Outgoing(u32),
    /// Byte `offset` of a coroutine's heap frame (WP-106, §7.6): the
    /// target addresses it from its coroutine frame base, and reports
    /// each access with `FnCx::coro_access`.
    Coro(u32),
}

impl FrameRef {
    /// The place `bytes` further on in the same area.
    #[must_use]
    pub const fn offset_by(self, bytes: u32) -> FrameRef {
        match self {
            FrameRef::Local(at) => FrameRef::Local(at + bytes),
            FrameRef::Incoming(at) => FrameRef::Incoming(at + bytes),
            FrameRef::Outgoing(at) => FrameRef::Outgoing(at + bytes),
            FrameRef::Coro(at) => FrameRef::Coro(at + bytes),
        }
    }
}

/// Where a body's stack slot is (§5.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotPlace {
    /// The slot itself is here: its address is the frame pointer plus a
    /// constant.
    Direct(FrameRef),
    /// An over-aligned slot: it was allocated dynamically by the entry
    /// code, and this pointer slot holds its address.
    Indirect(FrameRef),
}

/// A function's local area and the slots handed out from it. Reset, not
/// freed, between functions.
#[derive(Debug, Default)]
pub struct Frame {
    size: u32,
    stack_align: u32,
    /// Free spill slots by class (`Gpr`, `Fpr`), as local-area offsets.
    free_spills: [Vec<u32>; 2],
    /// Set at the first call of a `returns_twice` function: a spill slot
    /// released from then on is never handed out again (§4.3).
    keep_spills: bool,
    /// Instruction-lifetime slots (a `Mem`-class constant, the buffer of
    /// an unused in-memory result): `(offset, size)`, free and in use.
    free_temps: Vec<(u32, u32)>,
    live_temps: Vec<(u32, u32)>,
    /// The bytes alignment skipped and no slot uses yet: `(offset, len)`,
    /// at most [`MAX_HOLES`] of them (§5.1).
    holes: Vec<(u32, u32)>,
    /// Block slots (§5.1) by size class (1, 2, 4, 8 and 16 bytes), as
    /// local-area offsets in the order they were made.
    block_slots: [Vec<u32>; BLOCK_CLASSES],
    /// How many of each class's block slots the current block took: always
    /// the first ones of the list.
    block_taken: [usize; BLOCK_CLASSES],
}

/// The size classes of block slots: 1, 2, 4, 8 and 16 bytes.
const BLOCK_CLASSES: usize = 5;

/// The most holes a frame remembers: a bound on the search a value slot
/// makes; a hole that does not fit is never used, and costs nothing else.
const MAX_HOLES: usize = 16;

/// The largest value slot a hole is searched for: a register's bytes.
const MAX_HOLE_SLOT: u64 = 16;

impl Frame {
    /// An empty frame for a target whose stack alignment is `stack_align`
    /// bytes.
    pub fn new(stack_align: u32) -> Frame {
        Frame { stack_align, ..Frame::default() }
    }

    /// Empties the frame for the next function, keeping its buffers.
    pub fn reset(&mut self, stack_align: u32) {
        debug_assert!(stack_align.is_power_of_two(), "the stack alignment is a power of two");
        self.size = 0;
        self.stack_align = stack_align;
        self.free_spills[0].clear();
        self.free_spills[1].clear();
        self.keep_spills = false;
        self.free_temps.clear();
        self.live_temps.clear();
        self.holes.clear();
        for list in &mut self.block_slots {
            list.clear();
        }
        self.block_taken = [0; BLOCK_CLASSES];
    }

    /// The size of the local area so far, in bytes. Final once the
    /// function has been emitted; the target rounds it as its frame
    /// needs.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// The target's stack alignment.
    pub fn stack_align(&self) -> u32 {
        self.stack_align
    }

    /// A new slot of `size` bytes aligned to `align`, a power of two up to
    /// the stack alignment. Never reused.
    ///
    /// # Errors
    /// `Unsupported("limit:frame-size")` when the local area would exceed
    /// [`MAX_FRAME`]; `Internal` for an alignment that is not a power of
    /// two or exceeds the stack alignment.
    pub fn alloc(&mut self, size: u64, align: u32) -> CgResult<FrameRef> {
        if !align.is_power_of_two() || align > self.stack_align {
            return internal(format!(
                "a frame slot aligned to {align} with a stack alignment of {}",
                self.stack_align
            ));
        }
        let at = self.size.next_multiple_of(align);
        let end = match u64::from(at).checked_add(size) {
            Some(end) if end <= u64::from(MAX_FRAME) => end,
            _ => return Err(CgError::unsupported(LIMIT_FRAME_SIZE)),
        };
        if at > self.size && self.holes.len() < MAX_HOLES {
            self.holes.push((self.size, at - self.size));
        }
        self.size = end as u32;
        Ok(FrameRef::Local(at))
    }

    /// A new value slot (§5.1 "Value slots"): as [`Frame::alloc`], but in
    /// a hole alignment left when one fits (the lowest such place).
    /// Never reused by this call.
    ///
    /// # Errors
    /// As [`Frame::alloc`].
    pub fn alloc_value(&mut self, size: u64, align: u32) -> CgResult<FrameRef> {
        if size != 0 && size <= MAX_HOLE_SLOT && align.is_power_of_two() {
            let size = size as u32;
            let found = self.holes.iter().enumerate().find_map(|(index, &(at, len))| {
                let start = at.next_multiple_of(align);
                (start + size <= at + len).then_some((index, start))
            });
            if let Some((index, start)) = found {
                let (at, len) = self.holes[index];
                let (before, after) = (start - at, at + len - (start + size));
                match (before, after) {
                    (0, 0) => {
                        self.holes.swap_remove(index);
                    }
                    (0, _) => self.holes[index] = (start + size, after),
                    (_, 0) => self.holes[index] = (at, before),
                    _ => {
                        self.holes[index] = (at, before);
                        if self.holes.len() < MAX_HOLES {
                            self.holes.push((start + size, after));
                        }
                    }
                }
                return Ok(FrameRef::Local(start));
            }
        }
        self.alloc(size, align)
    }

    /// A block begins (§5.1): every block slot is free in it.
    #[inline]
    pub(crate) fn begin_block(&mut self) {
        self.block_taken = [0; BLOCK_CLASSES];
    }

    /// A block slot of `size` bytes aligned to `align` (§5.1): one not
    /// taken since the current block began, of the smallest size class
    /// that has one; a new slot of the request's class otherwise. It is
    /// the slot's until the block ends.
    ///
    /// The class of a request is the power of two that holds it, up to 16
    /// bytes, and its slots are aligned to that size (or the stack
    /// alignment, if smaller). Each class lists its slots in the order
    /// they were made and a block takes them in that order, so the slots a
    /// block holds are the first ones of each list and a count per class
    /// is the block's whole state: constant time, however many slots the
    /// function has. A larger request, or one aligned beyond its class, is
    /// a plain value slot, never reused.
    ///
    /// # Errors
    /// As [`Frame::alloc`].
    pub(crate) fn alloc_block(&mut self, size: u64, align: u32) -> CgResult<FrameRef> {
        let class_bytes = size.max(1).next_power_of_two();
        if class_bytes > 1 << (BLOCK_CLASSES - 1) || u64::from(align) > class_bytes {
            return self.alloc_value(size, align);
        }
        let class = class_bytes.trailing_zeros() as usize;
        for larger in class..BLOCK_CLASSES {
            let taken = self.block_taken[larger];
            if let Some(&at) = self.block_slots[larger].get(taken) {
                self.block_taken[larger] = taken + 1;
                return Ok(FrameRef::Local(at));
            }
        }
        let FrameRef::Local(at) =
            self.alloc_value(class_bytes, (class_bytes as u32).min(self.stack_align))?
        else {
            return internal("the allocator returned a slot outside the local area");
        };
        self.block_slots[class].push(at);
        self.block_taken[class] = self.block_slots[class].len();
        Ok(FrameRef::Local(at))
    }

    /// A spill slot for a register of `class` holding `bytes` bytes:
    /// a free one if there is one, a new one otherwise (§4.3).
    pub(crate) fn alloc_spill(&mut self, class: Class, bytes: u32) -> CgResult<FrameRef> {
        if let Some(at) = self.free_spills[spill_list(class)].pop() {
            return Ok(FrameRef::Local(at));
        }
        self.alloc_value(u64::from(bytes), bytes.min(self.stack_align))
    }

    /// Gives a spill slot back once its value's use has been emitted.
    pub(crate) fn free_spill(&mut self, class: Class, slot: FrameRef) {
        if self.keep_spills {
            return;
        }
        if let FrameRef::Local(at) = slot {
            self.free_spills[spill_list(class)].push(at);
        }
    }

    /// From here on no released spill slot is reused: a call of a
    /// `returns_twice` function is being emitted, and the code after it
    /// can run a second time and read a slot again (§4.3). Slots that are
    /// free now stay usable: nothing reads them.
    pub(crate) fn keep_spills(&mut self) {
        self.keep_spills = true;
    }

    /// A slot that lives until [`Frame::end_instruction`].
    pub(crate) fn alloc_temp(&mut self, size: u64) -> CgResult<FrameRef> {
        let Ok(size) = u32::try_from(size.max(1)) else {
            return Err(CgError::unsupported(LIMIT_FRAME_SIZE));
        };
        let found = self.free_temps.iter().position(|&(_, have)| have >= size);
        let slot = if let Some(index) = found {
            self.free_temps.swap_remove(index)
        } else {
            let FrameRef::Local(at) = self.alloc(u64::from(size), self.stack_align)? else {
                return internal("the allocator returned a slot outside the local area");
            };
            (at, size)
        };
        self.live_temps.push(slot);
        Ok(FrameRef::Local(slot.0))
    }

    /// Frees the instruction-lifetime slots.
    #[inline]
    pub(crate) fn end_instruction(&mut self) {
        if !self.live_temps.is_empty() {
            self.free_temps.append(&mut self.live_temps);
        }
    }
}

fn spill_list(class: Class) -> usize {
    usize::from(class == Class::Fpr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(r: CgResult<FrameRef>) -> u32 {
        match r {
            Ok(FrameRef::Local(at)) => at,
            other => panic!("not a local slot: {other:?}"),
        }
    }

    /// §5.1: offsets only grow, each slot is aligned as asked, and the
    /// area's size is the end of the last slot.
    #[test]
    fn slots_are_bumped_and_aligned() {
        let mut frame = Frame::new(16);
        assert_eq!(local(frame.alloc(16, 16)), 0);
        assert_eq!(local(frame.alloc(1, 1)), 16);
        assert_eq!(local(frame.alloc(4, 4)), 20);
        assert_eq!(local(frame.alloc(0, 8)), 24);
        assert_eq!(local(frame.alloc(8, 8)), 24);
        assert_eq!(local(frame.alloc(3, 1)), 32);
        assert_eq!(local(frame.alloc(16, 16)), 48);
        assert_eq!(frame.size(), 64);
        frame.reset(8);
        assert_eq!(frame.size(), 0);
        assert_eq!(local(frame.alloc(8, 8)), 0);
    }

    /// §5.1: an alignment above the stack alignment is not the local
    /// area's to give; §5.4, §10: a frame beyond the limit is a named
    /// limit, not a wrapped offset.
    #[test]
    fn limits_are_errors() {
        let mut frame = Frame::new(16);
        assert!(matches!(frame.alloc(8, 32), Err(CgError::Internal(_))));
        assert!(matches!(frame.alloc(8, 3), Err(CgError::Internal(_))));
        assert_eq!(local(frame.alloc(u64::from(MAX_FRAME) - 8, 8)), 0);
        assert_eq!(local(frame.alloc(8, 8)), MAX_FRAME - 8);
        assert_eq!(frame.alloc(1, 1), Err(CgError::unsupported(LIMIT_FRAME_SIZE)));
        assert_eq!(frame.alloc(u64::MAX, 1), Err(CgError::unsupported(LIMIT_FRAME_SIZE)));
        assert_eq!(frame.size(), MAX_FRAME);
    }

    /// §4.3: a spill slot is reused once its value's use has been
    /// emitted, by class.
    #[test]
    fn spill_slots_are_reused_by_class() {
        let mut frame = Frame::new(16);
        let a = frame.alloc_spill(Class::Gpr, 8).unwrap();
        let b = frame.alloc_spill(Class::Gpr, 8).unwrap();
        let v = frame.alloc_spill(Class::Fpr, 16).unwrap();
        assert_eq!((a, b, v), (FrameRef::Local(0), FrameRef::Local(8), FrameRef::Local(16)));
        frame.free_spill(Class::Gpr, a);
        assert_eq!(frame.alloc_spill(Class::Fpr, 16).unwrap(), FrameRef::Local(32));
        assert_eq!(frame.alloc_spill(Class::Gpr, 8).unwrap(), a);
        frame.free_spill(Class::Fpr, v);
        assert_eq!(frame.alloc_spill(Class::Fpr, 16).unwrap(), v);
        assert_eq!(frame.size(), 48);
    }

    /// §4.3: a spill slot that is occupied at a call of a `returns_twice`
    /// function is never reused; one that was free at the call is.
    #[test]
    fn spill_slots_live_at_a_returns_twice_call_are_kept() {
        let mut frame = Frame::new(16);
        let free_before = frame.alloc_spill(Class::Gpr, 8).unwrap();
        let occupied = frame.alloc_spill(Class::Gpr, 8).unwrap();
        frame.free_spill(Class::Gpr, free_before);
        frame.keep_spills();
        frame.free_spill(Class::Gpr, occupied);
        assert_eq!(frame.alloc_spill(Class::Gpr, 8).unwrap(), free_before);
        let fresh = frame.alloc_spill(Class::Gpr, 8).unwrap();
        assert_ne!(fresh, occupied);
        frame.free_spill(Class::Gpr, fresh);
        assert_ne!(frame.alloc_spill(Class::Gpr, 8).unwrap(), fresh);
    }

    /// §5.1 "Value slots" (WP-198): the bytes alignment skipped are
    /// filled by value slots that fit, lowest first; a body slot
    /// (`alloc`) never goes into a hole.
    #[test]
    fn value_slots_fill_alignment_holes() {
        let mut frame = Frame::new(16);
        assert_eq!(local(frame.alloc(1, 1)), 0);
        assert_eq!(local(frame.alloc(8, 8)), 8); // hole 1..8
        assert_eq!(local(frame.alloc(1, 1)), 16);
        assert_eq!(local(frame.alloc(16, 16)), 32); // hole 17..32
        assert_eq!(local(frame.alloc_value(4, 4)), 4);
        assert_eq!(local(frame.alloc_value(1, 1)), 1);
        assert_eq!(local(frame.alloc_value(2, 2)), 2);
        assert_eq!(local(frame.alloc_value(8, 8)), 24);
        assert_eq!(local(frame.alloc_value(8, 8)), 48); // no hole holds it
        assert_eq!(local(frame.alloc(1, 1)), 56);
        assert_eq!(local(frame.alloc_value(16, 16)), 64);
        assert_eq!(local(frame.alloc_value(1, 1)), 17);
        assert_eq!(frame.size(), 80);
    }

    /// §5.1 "Value slots" (WP-198): a block slot is the block's until the
    /// next block begins; then any block may take it again, its own size
    /// class first, then the next larger.
    #[test]
    fn block_slots_are_reused_in_every_block() {
        let mut frame = Frame::new(16);
        frame.begin_block();
        let big1 = local(frame.alloc_block(16, 8));
        let small = local(frame.alloc_block(1, 1));
        let big2 = local(frame.alloc_block(16, 8));
        assert!(big1 != small && small != big2 && big1 != big2);
        let size = frame.size();
        frame.begin_block();
        assert_eq!(local(frame.alloc_block(1, 1)), small);
        assert_eq!(local(frame.alloc_block(16, 8)), big1);
        assert_eq!(local(frame.alloc_block(8, 8)), big2);
        assert_eq!(frame.size(), size);
        let small2 = local(frame.alloc_block(1, 1));
        assert!(small2 != big1 && small2 != small && small2 != big2);
        let odd = local(frame.alloc_block(24, 8));
        frame.begin_block();
        assert_ne!(local(frame.alloc_block(24, 8)), odd); // no class: never reused
        frame.reset(16);
        frame.begin_block();
        assert_eq!(local(frame.alloc_block(4, 4)), 0);
    }

    /// §5.1: instruction-lifetime slots come back at the end of the
    /// instruction and are reused by size.
    #[test]
    fn temporary_slots_last_one_instruction() {
        let mut frame = Frame::new(16);
        let a = frame.alloc_temp(16).unwrap();
        let b = frame.alloc_temp(24).unwrap();
        assert_ne!(a, b);
        frame.end_instruction();
        assert_eq!(frame.alloc_temp(20).unwrap(), b);
        assert_eq!(frame.alloc_temp(8).unwrap(), a);
        let c = frame.alloc_temp(8).unwrap();
        assert!(c != a && c != b);
        assert_eq!(FrameRef::Local(8).offset_by(4), FrameRef::Local(12));
        assert_eq!(FrameRef::Incoming(8).offset_by(4), FrameRef::Incoming(12));
        assert_eq!(FrameRef::Outgoing(0).offset_by(4), FrameRef::Outgoing(4));
    }
}
