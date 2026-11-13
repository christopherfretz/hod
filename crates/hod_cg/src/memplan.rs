//! The memory-intrinsic plan (`spec/codegen.md` §7.3, "Memory
//! intrinsics", WP-117): whether a `memcpy`, `memmove` or `memset` is
//! nothing, inline accesses, an inline loop, a call, or refused. A pure
//! function of the operation and its length; each target emits the plan.

use crate::sym::RuntimeFn;

/// The longest constant length `memcpy` and `memset` copy or set inline:
/// at most four 16-byte accesses and the pieces of the rest, against a
/// call that displaces every live register.
pub const INLINE_MAX: u64 = 64;

/// The longest constant length `memmove` copies inline: every byte is
/// read before any is written, into the scratch registers (two 16-byte
/// vector registers at most).
pub const INLINE_MOVE_MAX: u64 = 32;

/// A memory intrinsic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemOp {
    /// `llvm.memcpy`.
    Copy,
    /// `llvm.memcpy.inline`: never a call.
    CopyInline,
    /// `llvm.memmove`.
    Move,
    /// `llvm.memset`.
    Set,
    /// `llvm.memset.inline`: never a call.
    SetInline,
}

/// What a memory intrinsic becomes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemPlan {
    /// A length of 0: no code (the pointers may be null).
    Nothing,
    /// Straight-line accesses touching exactly the `bytes` of the range
    /// (1 to [`INLINE_MAX`]; a `memmove` at most [`INLINE_MOVE_MAX`],
    /// every byte read before any is written).
    Inline {
        /// The constant length.
        bytes: u64,
    },
    /// A loop of accesses: an `.inline` form of a constant length above
    /// [`INLINE_MAX`].
    Loop {
        /// The constant length.
        bytes: u64,
    },
    /// A call of the routine, with the length as given (a variable one
    /// zero-extended to 64 bits).
    Call(RuntimeFn),
    /// An `.inline` form of a variable length: the target refuses it by
    /// its own name.
    VariableInline,
}

/// The plan for `op` of `len` bytes (`None`: not a constant).
pub fn mem_plan(op: MemOp, len: Option<u64>) -> MemPlan {
    let (func, limit) = match op {
        MemOp::Copy | MemOp::CopyInline => (RuntimeFn::Memcpy, INLINE_MAX),
        MemOp::Move => (RuntimeFn::Memmove, INLINE_MOVE_MAX),
        MemOp::Set | MemOp::SetInline => (RuntimeFn::Memset, INLINE_MAX),
    };
    let inline = matches!(op, MemOp::CopyInline | MemOp::SetInline);
    match len {
        None if inline => MemPlan::VariableInline,
        Some(0) => MemPlan::Nothing,
        Some(bytes) if bytes <= limit => MemPlan::Inline { bytes },
        Some(bytes) if inline => MemPlan::Loop { bytes },
        // Longer, or a variable length.
        None | Some(_) => MemPlan::Call(func),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §7.3 "Memory intrinsics": the plan at each threshold.
    #[test]
    fn plans_at_the_thresholds() {
        use MemOp as O;
        use MemPlan as P;
        use RuntimeFn as R;
        let rows: [(MemOp, Option<u64>, MemPlan); 19] = [
            (O::Copy, Some(0), P::Nothing),
            (O::Copy, Some(1), P::Inline { bytes: 1 }),
            (O::Copy, Some(64), P::Inline { bytes: 64 }),
            (O::Copy, Some(65), P::Call(R::Memcpy)),
            (O::Copy, None, P::Call(R::Memcpy)),
            (O::CopyInline, Some(0), P::Nothing),
            (O::CopyInline, Some(64), P::Inline { bytes: 64 }),
            (O::CopyInline, Some(65), P::Loop { bytes: 65 }),
            (O::CopyInline, None, P::VariableInline),
            (O::Move, Some(0), P::Nothing),
            (O::Move, Some(32), P::Inline { bytes: 32 }),
            (O::Move, Some(33), P::Call(R::Memmove)),
            (O::Move, None, P::Call(R::Memmove)),
            (O::Set, Some(64), P::Inline { bytes: 64 }),
            (O::Set, Some(65), P::Call(R::Memset)),
            (O::Set, None, P::Call(R::Memset)),
            (O::SetInline, Some(64), P::Inline { bytes: 64 }),
            (O::SetInline, Some(u64::MAX), P::Loop { bytes: u64::MAX }),
            (O::SetInline, None, P::VariableInline),
        ];
        for (op, len, plan) in rows {
            assert_eq!(mem_plan(op, len), plan, "{op:?} of {len:?}");
        }
    }
}
