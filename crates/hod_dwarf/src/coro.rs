//! The locations of a coroutine body's variables (`spec/dwarf.md` §6.16,
//! design 0005 §3.2, WP-108).
//!
//! hod compiles a coroutine once (`spec/codegen.md` §7.6): the ramp and
//! every resumed activation run the same code, with the same machine
//! frame, and what lives across a suspend point is in a heap frame. The
//! machine frame has a **frame word** that holds the heap frame's address:
//! 0 from the prologue until `llvm.coro.begin`, then the address, in the
//! ramp and in every activation a stub entered. A variable's location is
//! one DWARF expression that holds at every pc of the body, built on that
//! word, so it needs no location list and does not depend on the register
//! the code addresses the heap through:
//!
//! - in the heap: `DW_OP_fbreg word, DW_OP_deref, DW_OP_plus_uconst o`,
//!   the expression of LLVM's split clones;
//! - copied at `coro.begin` (a parameter's slot, the promise): on the
//!   machine stack while the word is 0, in the heap after;
//! - a parameter on the machine stack only (an alloca marked
//!   `!coro.outside.frame`): there in the ramp (mode word 0), at address
//!   0 in a resumed activation, whose stack slot the ramp never wrote, so
//!   that a debugger says it cannot read it instead of showing another
//!   activation's bytes.

use crate::leb::{write_sleb, write_uleb};

const DW_OP_FBREG: u8 = 0x91;
const DW_OP_DEREF: u8 = 0x06;
const DW_OP_DUP: u8 = 0x12;
const DW_OP_DROP: u8 = 0x13;
const DW_OP_PLUS_UCONST: u8 = 0x23;
const DW_OP_SKIP: u8 = 0x2f;
const DW_OP_BRA: u8 = 0x28;
const DW_OP_LIT0: u8 = 0x30;

/// Where a variable of a coroutine body is ([`crate::LocalPlace::Coro`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoroPlace {
    /// The frame-base offset of the frame word: 0 before `coro.begin`,
    /// the heap frame's address after it, in every activation.
    pub word: i64,
    /// Where the variable's address is.
    pub at: CoroAt,
    /// The address found holds the variable's address (a declared value
    /// homed there) rather than being it.
    pub indirect: bool,
}

/// The three places of [`CoroPlace`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoroAt {
    /// At this offset of the heap frame.
    Heap(u32),
    /// Copied at `coro.begin`: at frame-base offset `stack` while the
    /// frame word is 0, at heap offset `heap` after.
    Copied {
        /// The heap offset.
        heap: u32,
        /// The machine-stack place before `coro.begin`.
        stack: i64,
    },
    /// On the machine stack at `stack`, written by the ramp only: valid
    /// while the mode word (frame-base offset `mode`) is 0.
    Ramp {
        /// The machine-stack place.
        stack: i64,
        /// The frame-base offset of the mode word.
        mode: i64,
    },
}

fn fbreg(out: &mut Vec<u8>, offset: i64) {
    out.push(DW_OP_FBREG);
    write_sleb(out, offset);
}

fn plus(out: &mut Vec<u8>, offset: u32) {
    if offset != 0 {
        out.push(DW_OP_PLUS_UCONST);
        write_uleb(out, u64::from(offset));
    }
}

/// A forward branch `op` over the `len` bytes that follow it.
fn branch(out: &mut Vec<u8>, op: u8, len: usize) {
    out.push(op);
    out.extend_from_slice(&(len as i16).to_le_bytes());
}

/// Appends the location of `place` to `out` (before the variable's own
/// `DIExpression` ops).
pub(crate) fn location(out: &mut Vec<u8>, place: CoroPlace) {
    match place.at {
        CoroAt::Heap(heap) => {
            fbreg(out, place.word);
            out.push(DW_OP_DEREF);
            plus(out, heap);
        }
        CoroAt::Copied { heap, stack } => {
            // word, deref, dup, bra HEAP, drop, fbreg stack, skip END,
            // HEAP: plus_uconst heap, END:
            let mut on_stack = vec![DW_OP_DROP];
            fbreg(&mut on_stack, stack);
            let mut in_heap = Vec::new();
            plus(&mut in_heap, heap);
            fbreg(out, place.word);
            out.extend_from_slice(&[DW_OP_DEREF, DW_OP_DUP]);
            branch(out, DW_OP_BRA, on_stack.len() + 3);
            out.extend_from_slice(&on_stack);
            branch(out, DW_OP_SKIP, in_heap.len());
            out.extend_from_slice(&in_heap);
        }
        CoroAt::Ramp { stack, mode } => {
            // mode, deref, bra ELSEWHERE, fbreg stack, skip END,
            // ELSEWHERE: lit0, END:
            let mut on_stack = Vec::new();
            fbreg(&mut on_stack, stack);
            fbreg(out, mode);
            out.push(DW_OP_DEREF);
            branch(out, DW_OP_BRA, on_stack.len() + 3);
            out.extend_from_slice(&on_stack);
            branch(out, DW_OP_SKIP, 1);
            out.push(DW_OP_LIT0);
        }
    }
    if place.indirect {
        out.push(DW_OP_DEREF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(place: CoroPlace) -> Vec<u8> {
        let mut out = Vec::new();
        location(&mut out, place);
        out
    }

    #[test]
    fn a_heap_variable_is_the_word_plus_its_offset() {
        let place = CoroPlace { word: -8, at: CoroAt::Heap(0x18), indirect: false };
        assert_eq!(bytes(place), [0x91, 0x78, 0x06, 0x23, 0x18]);
        let place = CoroPlace { word: 16, at: CoroAt::Heap(0x18), indirect: true };
        assert_eq!(bytes(place), [0x91, 0x10, 0x06, 0x23, 0x18, 0x06]);
    }

    #[test]
    fn a_copied_variable_branches_on_the_word() {
        let place =
            CoroPlace { word: 16, at: CoroAt::Copied { heap: 20, stack: 28 }, indirect: false };
        assert_eq!(
            bytes(place),
            [
                0x91, 0x10, 0x06, 0x12, // word, deref, dup
                0x28, 6, 0, // bra over drop, fbreg 28, skip
                0x13, 0x91, 0x1c, // drop, fbreg 28
                0x2f, 2, 0, // skip over plus_uconst
                0x23, 20, // plus_uconst 20
            ]
        );
    }

    #[test]
    fn a_ramp_parameter_is_address_zero_when_resumed() {
        let place =
            CoroPlace { word: 16, at: CoroAt::Ramp { stack: 28, mode: 24 }, indirect: false };
        assert_eq!(bytes(place), [0x91, 0x18, 0x06, 0x28, 5, 0, 0x91, 0x1c, 0x2f, 1, 0, 0x30]);
    }
}
