//! The flat byte memory of a run (`spec/interp.md` §4).
//!
//! Memory is a list of allocations ordered by address. Addresses are plain
//! 64-bit integers handed out upwards and never handed out twice, so a
//! pointer into a frame that has returned names nothing live. The bytes of
//! every allocation, and one "written" flag per byte, live in one arena
//! that grows and shrinks like a stack: stack allocations die in the
//! reverse of the order they were made.

use std::cell::Cell;

use hod_ir::SymId;

use crate::error::Trap;

/// What an allocation is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AllocKind {
    /// A writable module variable.
    Data,
    /// A `constant` module variable.
    Constant,
    /// A stack slot or a dynamic `alloca` of a live frame.
    Stack,
    /// A function: one address, no bytes.
    Function(SymId),
    /// A variable without an initializer (a declaration): one address, no
    /// bytes. What it holds is not the program's to know.
    Extern(SymId),
    /// The addresses of frames that have returned.
    Dead,
}

#[derive(Clone, Copy, Debug)]
struct Alloc {
    base: u64,
    /// Bytes of data; for `Function` and `Dead`, the addresses covered.
    size: u64,
    align: u64,
    /// Position of the data in the arena.
    at: usize,
    kind: AllocKind,
}

/// A point to which the stack can be unwound.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Mark {
    allocs: usize,
    bytes: usize,
}

/// The memory of a run. A host function gets it to read and write what
/// its pointer arguments point to (§6).
#[derive(Clone, Debug)]
pub struct Memory {
    allocs: Vec<Alloc>,
    bytes: Vec<u8>,
    written: Vec<bool>,
    /// The lowest address not handed out yet.
    next: u64,
    gap: u64,
    limit: u64,
    peak: u64,
    /// The declared variable the last refused access met, if it met one.
    extern_hit: Cell<Option<SymId>>,
}

impl Memory {
    /// An empty memory whose first allocation is above `base`, with at
    /// least `gap` unused addresses between allocations and at most
    /// `limit` live bytes.
    pub(crate) fn new(base: u64, gap: u64, limit: u64) -> Memory {
        Memory {
            allocs: Vec::new(),
            bytes: Vec::new(),
            written: Vec::new(),
            next: base,
            gap: gap.max(1),
            limit,
            peak: 0,
            extern_hit: Cell::new(None),
        }
    }

    /// Bytes in live allocations.
    pub fn live_bytes(&self) -> u64 {
        self.bytes.len() as u64
    }

    /// The most bytes that were ever live at once.
    pub fn peak_bytes(&self) -> u64 {
        self.peak
    }

    /// The `len` bytes at `addr`. Every one must be inside one live data
    /// allocation and have been written.
    ///
    /// # Errors
    /// `OutOfBounds`, `UseAfterFree` or `UninitializedLoad`.
    pub fn read(&self, addr: u64, len: u64) -> Result<&[u8], Trap> {
        self.load(addr, len, 1)
    }

    /// Writes `data` at `addr`, inside one live writable allocation.
    ///
    /// # Errors
    /// `OutOfBounds`, `UseAfterFree` or `WriteToConstant`.
    pub fn write(&mut self, addr: u64, data: &[u8]) -> Result<(), Trap> {
        self.store(addr, data, 1)
    }

    /// A new allocation of `size` uninitialized bytes aligned to `align`.
    pub(crate) fn alloc(&mut self, size: u64, align: u64, kind: AllocKind) -> Result<u64, Trap> {
        let live = self.bytes.len() as u64;
        if size > self.limit.saturating_sub(live) {
            return Err(Trap::Memory);
        }
        let at = self.bytes.len();
        let end = usize::try_from(size).ok().and_then(|size| at.checked_add(size));
        let end = end.ok_or(Trap::Memory)?;
        let base = self.place(size, align)?;
        self.bytes.resize(end, 0);
        self.written.resize(end, false);
        self.peak = self.peak.max(end as u64);
        self.allocs.push(Alloc { base, size, align, at, kind });
        Ok(base)
    }

    /// The address of a function: an allocation without bytes, aligned
    /// to the function's `align` (the Itanium C++ ABI of x86-64 tests the
    /// low bit of a member function's address).
    pub(crate) fn alloc_function(&mut self, sym: SymId, align: u64) -> Result<u64, Trap> {
        let base = self.place(1, align)?;
        self.allocs.push(Alloc { base, size: 1, align, at: 0, kind: AllocKind::Function(sym) });
        Ok(base)
    }

    /// The address of a declared variable: an allocation without bytes.
    pub(crate) fn alloc_extern(&mut self, sym: SymId) -> Result<u64, Trap> {
        let base = self.place(1, 1)?;
        self.allocs.push(Alloc { base, size: 1, align: 1, at: 0, kind: AllocKind::Extern(sym) });
        Ok(base)
    }

    /// A fresh address that no allocation covers (a block address, an
    /// exception): an identity, not memory. An access through it traps.
    pub(crate) fn reserve(&mut self) -> Result<u64, Trap> {
        self.place(1, 1)
    }

    /// The declared variable a refused access since the last call met,
    /// and forgets it.
    pub(crate) fn take_extern_hit(&self) -> Option<SymId> {
        self.extern_hit.take()
    }

    /// Takes the next addresses: a gap, then `size` (at least one) aligned
    /// to `align`.
    fn place(&mut self, size: u64, align: u64) -> Result<u64, Trap> {
        let mask = align.max(1) - 1;
        let base = self
            .next
            .checked_add(self.gap)
            .and_then(|start| start.checked_add(mask))
            .map(|start| start & !mask);
        let end = base.and_then(|base| base.checked_add(size.max(1)));
        let (Some(base), Some(end)) = (base, end) else {
            return Err(Trap::Memory);
        };
        self.next = end;
        Ok(base)
    }

    /// The state to return to with [`Memory::release`].
    pub(crate) fn mark(&self) -> Mark {
        Mark { allocs: self.allocs.len(), bytes: self.bytes.len() }
    }

    /// Ends every allocation made since `mark`. Their addresses stay
    /// taken, as one dead range.
    pub(crate) fn release(&mut self, mark: Mark) {
        let Some(first) = self.allocs.get(mark.allocs) else {
            return;
        };
        let base = first.base;
        self.allocs.truncate(mark.allocs);
        self.bytes.truncate(mark.bytes);
        self.written.truncate(mark.bytes);
        match self.allocs.last_mut() {
            Some(last) if last.kind == AllocKind::Dead => last.size = self.next - last.base,
            _ => self.allocs.push(Alloc {
                base,
                size: self.next - base,
                align: 1,
                at: 0,
                kind: AllocKind::Dead,
            }),
        }
    }

    fn find(&self, addr: u64) -> Option<&Alloc> {
        let after = self.allocs.partition_point(|alloc| alloc.base <= addr);
        after.checked_sub(1).map(|index| &self.allocs[index])
    }

    /// The function whose address is exactly `addr`.
    pub(crate) fn function_at(&self, addr: u64) -> Option<SymId> {
        match self.find(addr) {
            Some(&Alloc { base, kind: AllocKind::Function(sym), .. }) if base == addr => Some(sym),
            _ => None,
        }
    }

    /// Checks an access of `len` bytes at `addr` with alignment `align`
    /// and returns where its bytes are in the arena (§4).
    pub(crate) fn check(
        &self,
        addr: u64,
        len: u64,
        align: u64,
        write: bool,
    ) -> Result<usize, Trap> {
        let alloc = self.find(addr).ok_or(Trap::OutOfBounds)?;
        let offset = addr - alloc.base;
        match alloc.kind {
            AllocKind::Dead if offset < alloc.size => return Err(Trap::UseAfterFree),
            AllocKind::Dead | AllocKind::Function(_) => return Err(Trap::OutOfBounds),
            AllocKind::Extern(sym) => {
                self.extern_hit.set(Some(sym));
                return Err(Trap::OutOfBounds);
            }
            AllocKind::Data | AllocKind::Constant | AllocKind::Stack => {}
        }
        if offset > alloc.size || len > alloc.size - offset {
            return Err(Trap::OutOfBounds);
        }
        if align > alloc.align || !addr.is_multiple_of(align.max(1)) {
            return Err(Trap::Misaligned);
        }
        if write && alloc.kind == AllocKind::Constant {
            return Err(Trap::WriteToConstant);
        }
        Ok(alloc.at + offset as usize)
    }

    /// The bytes of a checked access, written or not.
    pub(crate) fn raw(&self, at: usize, len: usize) -> &[u8] {
        &self.bytes[at..at + len]
    }

    /// Whether every byte of a checked access was written.
    pub(crate) fn is_written(&self, at: usize, len: usize) -> bool {
        self.written[at..at + len].iter().all(|&written| written)
    }

    /// Marks the bytes of a checked access written or unwritten.
    pub(crate) fn set_written(&mut self, at: usize, len: usize, written: bool) {
        self.written[at..at + len].fill(written);
    }

    /// Writes `data` into a checked access and marks it written.
    pub(crate) fn put(&mut self, at: usize, data: &[u8]) {
        self.bytes[at..at + data.len()].copy_from_slice(data);
        self.written[at..at + data.len()].fill(true);
    }

    /// A load of `len` written bytes.
    pub(crate) fn load(&self, addr: u64, len: u64, align: u64) -> Result<&[u8], Trap> {
        if len == 0 {
            return Ok(&[]);
        }
        let at = self.check(addr, len, align, false)?;
        let len = len as usize;
        if !self.is_written(at, len) {
            return Err(Trap::UninitializedLoad);
        }
        Ok(self.raw(at, len))
    }

    /// A store of `data`.
    pub(crate) fn store(&mut self, addr: u64, data: &[u8], align: u64) -> Result<(), Trap> {
        if data.is_empty() {
            return Ok(());
        }
        let at = self.check(addr, data.len() as u64, align, true)?;
        self.put(at, data);
        Ok(())
    }

    /// Copies `len` bytes and their written flags; the ranges may overlap.
    pub(crate) fn copy(&mut self, dst: (u64, u64), src: (u64, u64), len: u64) -> Result<(), Trap> {
        if len == 0 {
            return Ok(());
        }
        let from = self.check(src.0, len, src.1, false)?;
        let to = self.check(dst.0, len, dst.1, true)?;
        let len = len as usize;
        self.bytes.copy_within(from..from + len, to);
        self.written.copy_within(from..from + len, to);
        Ok(())
    }

    /// Sets `len` bytes to `byte`.
    pub(crate) fn fill(&mut self, dst: (u64, u64), byte: u8, len: u64) -> Result<(), Trap> {
        if len == 0 {
            return Ok(());
        }
        let at = self.check(dst.0, len, dst.1, true)?;
        let len = len as usize;
        self.bytes[at..at + len].fill(byte);
        self.written[at..at + len].fill(true);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Memory {
        Memory::new(0x1000, 16, 1 << 20)
    }

    /// §4: allocations are apart, aligned as asked, and an access must be
    /// inside one of them.
    #[test]
    fn bounds_and_gaps() {
        let mut m = memory();
        let a = m.alloc(8, 8, AllocKind::Stack).unwrap();
        let b = m.alloc(3, 1, AllocKind::Stack).unwrap();
        let z = m.alloc(0, 1, AllocKind::Stack).unwrap();
        assert!(a >= 0x1010 && a.is_multiple_of(8));
        assert!(b >= a + 8 + 16 && z >= b + 3 + 16);
        m.write(a, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(m.read(a + 6, 2).unwrap(), [7, 8]);
        assert_eq!(m.read(a + 8, 0).unwrap(), [0u8; 0]);
        assert_eq!(m.read(a + 7, 2), Err(Trap::OutOfBounds));
        assert_eq!(m.read(a + 8, 1), Err(Trap::OutOfBounds));
        assert_eq!(m.read(a - 1, 1), Err(Trap::OutOfBounds));
        assert_eq!(m.read(0, 1), Err(Trap::OutOfBounds));
        assert_eq!(m.read(u64::MAX, 1), Err(Trap::OutOfBounds));
        assert_eq!(m.write(b + 2, &[1, 2]), Err(Trap::OutOfBounds));
        assert_eq!(m.write(z, &[1]), Err(Trap::OutOfBounds));
        assert_eq!(m.write(z, &[]), Ok(()));
        assert_eq!(m.live_bytes(), 11);
    }

    /// §4: a byte is readable only once written; copies carry the flag.
    #[test]
    fn written_flags() {
        let mut m = memory();
        let a = m.alloc(4, 1, AllocKind::Stack).unwrap();
        let b = m.alloc(4, 1, AllocKind::Stack).unwrap();
        assert_eq!(m.read(a, 1), Err(Trap::UninitializedLoad));
        m.write(a + 1, &[9]).unwrap();
        assert_eq!(m.read(a + 1, 1).unwrap(), [9]);
        assert_eq!(m.read(a, 2), Err(Trap::UninitializedLoad));
        m.copy((b, 1), (a, 1), 4).unwrap();
        assert_eq!(m.read(b + 1, 1).unwrap(), [9]);
        assert_eq!(m.read(b + 2, 1), Err(Trap::UninitializedLoad));
        m.fill((b, 1), 7, 4).unwrap();
        assert_eq!(m.read(b, 4).unwrap(), [7; 4]);
        // Overlapping copies behave like `memmove`.
        m.write(a, &[1, 2, 3, 4]).unwrap();
        m.copy((a + 1, 1), (a, 1), 3).unwrap();
        assert_eq!(m.read(a, 4).unwrap(), [1, 1, 2, 3]);
    }

    /// §4: the alignment an access states must be one the allocation
    /// guarantees, whatever the address happens to be.
    #[test]
    fn alignment_is_what_the_allocation_guarantees() {
        let mut m = memory();
        let a = m.alloc(16, 4, AllocKind::Stack).unwrap();
        assert!(m.check(a, 4, 4, false).is_ok());
        assert!(m.check(a + 4, 4, 4, true).is_ok());
        assert_eq!(m.check(a + 2, 4, 4, false), Err(Trap::Misaligned));
        assert_eq!(m.check(a, 8, 8, false), Err(Trap::Misaligned));
        assert!(m.check(a + 1, 8, 1, false).is_ok());
        // Bounds are judged first.
        assert_eq!(m.check(a + 14, 4, 4, false), Err(Trap::OutOfBounds));
    }

    /// §4: released stack memory is dead for good; its addresses are not
    /// reused, and consecutive dead ranges are one entry.
    #[test]
    fn release_leaves_dead_addresses() {
        let mut m = memory();
        let keep = m.alloc(4, 4, AllocKind::Data).unwrap();
        let ro = m.alloc(4, 4, AllocKind::Constant).unwrap();
        let entries = m.allocs.len();
        let mut dead = Vec::new();
        for _ in 0..3 {
            let mark = m.mark();
            let a = m.alloc(8, 8, AllocKind::Stack).unwrap();
            let b = m.alloc(8, 8, AllocKind::Stack).unwrap();
            m.write(a, &[1]).unwrap();
            m.release(mark);
            assert_eq!(m.read(a, 1), Err(Trap::UseAfterFree));
            assert_eq!(m.write(b + 7, &[1]), Err(Trap::UseAfterFree));
            assert!(dead.iter().all(|&old| old < a), "addresses are never reused");
            dead.push(a);
        }
        assert_eq!(m.allocs.len(), entries + 1);
        assert_eq!(m.read(dead[0], 1), Err(Trap::UseAfterFree));
        assert_eq!(m.live_bytes(), 8);
        assert_eq!(m.peak_bytes(), 24);
        // A release with nothing to release changes nothing.
        let mark = m.mark();
        m.release(mark);
        assert_eq!(m.allocs.len(), entries + 1);
        m.write(keep, &[1, 2, 3, 4]).unwrap();
        assert_eq!(m.write(ro, &[1]), Err(Trap::WriteToConstant));
        assert_eq!(m.fill((ro, 1), 0, 1), Err(Trap::WriteToConstant));
        assert_eq!(m.copy((ro, 1), (keep, 1), 1), Err(Trap::WriteToConstant));
    }

    /// §4, §7: functions have addresses and no bytes; the byte limit and
    /// the end of the address space are `Memory` traps.
    #[test]
    fn functions_and_limits() {
        let mut m = Memory::new(0x1000, 1, 32);
        let f = m.alloc_function(SymId::new(3), 1).unwrap();
        let g = m.alloc_function(SymId::new(4), 1).unwrap();
        assert_ne!(f, g);
        // WP-138: a function's `align` holds for its address.
        let mut aligned = Memory::new(0x1001, 1, 32);
        assert_eq!(aligned.alloc_function(SymId::new(5), 16).unwrap() % 16, 0);
        assert_eq!(m.function_at(f), Some(SymId::new(3)));
        assert_eq!(m.function_at(f + 1), None);
        assert_eq!(m.function_at(0), None);
        assert_eq!(m.read(f, 1), Err(Trap::OutOfBounds));
        let a = m.alloc(24, 1, AllocKind::Stack).unwrap();
        assert_eq!(m.function_at(a), None);
        assert_eq!(m.alloc(9, 1, AllocKind::Stack), Err(Trap::Memory));
        assert!(m.alloc(8, 1, AllocKind::Stack).is_ok());
        assert_eq!(m.alloc(u64::MAX, 1, AllocKind::Stack), Err(Trap::Memory));

        let mut top = Memory::new(u64::MAX - 8, 1, 1 << 20);
        assert!(top.alloc(4, 1, AllocKind::Stack).is_ok());
        assert_eq!(top.alloc(4, 1, AllocKind::Stack), Err(Trap::Memory));
        assert_eq!(top.alloc_function(SymId::new(0), 1), Ok(u64::MAX - 2));
        assert_eq!(top.alloc_function(SymId::new(0), 1), Err(Trap::Memory));
    }
}
