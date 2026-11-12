//! Labels and fixups: `spec/codegen.md` §6.2, §6.3.
//!
//! A label is bound to a code offset when what it names is emitted. Until
//! then it heads a list of fixups: places in the code that refer to it and
//! are patched when it is bound. A reference to a bound label is patched
//! at once. What a fixup's field looks like is the target's business: the
//! table only keeps `(offset, kind)` and hands both to a patch function
//! with the displacement.

/// A label of the function being compiled: a block, the epilogue, a tail
/// exit, a data island, or one a lowering made for itself.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Label(pub(crate) u32);

/// What kind of field a fixup patches. The numbering is the target's.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FixupKind(pub u8);

/// A fixup's field cannot hold the displacement (§6.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutOfRange;

const UNBOUND: u32 = u32::MAX;
const END: u32 = u32::MAX;

#[derive(Clone, Copy, Debug)]
struct Fixup {
    at: u32,
    kind: FixupKind,
    /// The next fixup of the same label, or [`END`].
    next: u32,
}

/// The labels of one function. Reset, not freed, between functions.
#[derive(Debug, Default)]
pub struct Labels {
    /// Per label: the code offset it is bound to, or [`UNBOUND`].
    offset: Vec<u32>,
    /// Per label: its first pending fixup, or [`END`].
    head: Vec<u32>,
    fixups: Vec<Fixup>,
    /// Fixups not yet patched.
    pending: u32,
}

impl Labels {
    /// Empties the table and makes `count` unbound labels, numbered from
    /// zero.
    pub fn reset(&mut self, count: u32) {
        self.offset.clear();
        self.offset.resize(count as usize, UNBOUND);
        self.head.clear();
        self.head.resize(count as usize, END);
        self.fixups.clear();
        self.pending = 0;
    }

    /// A new unbound label.
    pub fn create(&mut self) -> Label {
        self.offset.push(UNBOUND);
        self.head.push(END);
        Label(self.offset.len() as u32 - 1)
    }

    /// The number of labels.
    pub fn len(&self) -> u32 {
        self.offset.len() as u32
    }

    /// Whether there is no label.
    pub fn is_empty(&self) -> bool {
        self.offset.is_empty()
    }

    /// The offset `label` is bound to, if it is bound.
    pub fn offset(&self, label: Label) -> Option<u32> {
        self.offset.get(label.0 as usize).copied().filter(|&at| at != UNBOUND)
    }

    /// Whether anything refers to `label` and has not been patched.
    pub fn has_pending(&self, label: Label) -> bool {
        self.head.get(label.0 as usize).is_some_and(|&head| head != END)
    }

    /// The number of fixups not yet patched, over all labels.
    pub fn pending(&self) -> u32 {
        self.pending
    }

    /// Records that the field of `kind` at offset `at` refers to `label`.
    /// Patched now if the label is bound, when it is bound otherwise.
    ///
    /// `patch(at, kind, displacement)` writes the field; the displacement
    /// is the label's offset minus `at`.
    ///
    /// # Errors
    /// [`OutOfRange`] when `patch` answers `false`.
    pub fn refer(
        &mut self,
        label: Label,
        at: u32,
        kind: FixupKind,
        patch: impl FnOnce(u32, FixupKind, i64) -> bool,
    ) -> Result<(), OutOfRange> {
        let index = label.0 as usize;
        debug_assert!(index < self.offset.len(), "a label of another function");
        let target = self.offset[index];
        if target != UNBOUND {
            return patch(at, kind, i64::from(target) - i64::from(at))
                .then_some(())
                .ok_or(OutOfRange);
        }
        self.fixups.push(Fixup { at, kind, next: self.head[index] });
        self.head[index] = self.fixups.len() as u32 - 1;
        self.pending += 1;
        Ok(())
    }

    /// Binds `label` to `offset` and patches every fixup waiting for it.
    /// Returns `Ok(false)`, changing nothing, if the label was bound
    /// already.
    ///
    /// # Errors
    /// [`OutOfRange`] when `patch` answers `false` for one of them.
    pub fn bind(
        &mut self,
        label: Label,
        offset: u32,
        mut patch: impl FnMut(u32, FixupKind, i64) -> bool,
    ) -> Result<bool, OutOfRange> {
        let index = label.0 as usize;
        debug_assert!(index < self.offset.len(), "a label of another function");
        if self.offset[index] != UNBOUND {
            return Ok(false);
        }
        self.offset[index] = offset;
        let mut next = std::mem::replace(&mut self.head[index], END);
        while next != END {
            let fixup = self.fixups[next as usize];
            self.pending -= 1;
            if !patch(fixup.at, fixup.kind, i64::from(offset) - i64::from(fixup.at)) {
                return Err(OutOfRange);
            }
            next = fixup.next;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field of kind 0 holds a signed byte, of kind 1 a signed 32-bit
    /// displacement.
    fn patch(code: &mut [u8], at: u32, kind: FixupKind, disp: i64) -> bool {
        let at = at as usize;
        if kind.0 == 0 {
            let Ok(short) = i8::try_from(disp) else { return false };
            code[at] = short as u8;
        } else {
            let Ok(long) = i32::try_from(disp) else { return false };
            code[at..at + 4].copy_from_slice(&long.to_le_bytes());
        }
        true
    }

    /// §6.2: a forward reference is patched when its label is bound, a
    /// backward one at once; both get `label - at`.
    #[test]
    fn forward_and_backward_references() {
        let mut code = vec![0u8; 64];
        let mut labels = Labels::default();
        labels.reset(2);
        let (a, b) = (Label(0), Label(1));
        labels.refer(a, 4, FixupKind(0), |at, k, d| patch(&mut code, at, k, d)).unwrap();
        labels.refer(a, 8, FixupKind(1), |at, k, d| patch(&mut code, at, k, d)).unwrap();
        labels.refer(b, 12, FixupKind(0), |at, k, d| patch(&mut code, at, k, d)).unwrap();
        assert!(labels.has_pending(a) && labels.has_pending(b));
        assert_eq!(labels.pending(), 3);
        assert_eq!(code[4], 0);
        assert!(labels.bind(a, 40, |at, k, d| patch(&mut code, at, k, d)).unwrap());
        assert_eq!(code[4], 36);
        assert_eq!(code[8..12], 32i32.to_le_bytes());
        assert_eq!(code[12], 0, "the other label's fixup is untouched");
        assert!(!labels.has_pending(a) && labels.has_pending(b));
        assert_eq!(labels.offset(a), Some(40));
        assert_eq!(labels.offset(b), None);
        // Backward: patched at once, with a negative displacement.
        labels.refer(a, 50, FixupKind(0), |at, k, d| patch(&mut code, at, k, d)).unwrap();
        assert_eq!(code[50] as i8, -10);
        assert_eq!(labels.pending(), 1);
        // Binding twice changes nothing.
        assert!(!labels.bind(a, 60, |_, _, _| panic!("no fixup is pending")).unwrap());
        assert_eq!(labels.offset(a), Some(40));
        let c = labels.create();
        assert_eq!((c, labels.len()), (Label(2), 3));
        assert!(!labels.has_pending(c));
    }

    /// §6.3: a field that cannot hold the displacement is reported, for a
    /// reference to a bound label and at binding alike.
    #[test]
    fn out_of_range_is_reported() {
        let mut code = vec![0u8; 8];
        let mut labels = Labels::default();
        labels.reset(2);
        labels.bind(Label(0), 0, |_, _, _| true).unwrap();
        let far = labels.refer(Label(0), 200, FixupKind(0), |_, k, d| {
            assert_eq!((k, d), (FixupKind(0), -200));
            false
        });
        assert_eq!(far, Err(OutOfRange));
        labels.refer(Label(1), 0, FixupKind(0), |at, k, d| patch(&mut code, at, k, d)).unwrap();
        let bound = labels.bind(Label(1), 128, |at, k, d| patch(&mut code, at, k, d));
        assert_eq!(bound, Err(OutOfRange));
        // The table is reusable after a reset.
        labels.reset(1);
        assert_eq!((labels.len(), labels.pending()), (1, 0));
        assert_eq!(labels.offset(Label(0)), None);
        assert!(!labels.is_empty());
    }
}
