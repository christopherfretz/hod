//! Errors and limits: `spec/codegen.md` §10.

use std::fmt;

/// Why a function could not be compiled.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CgError {
    /// A named limit that can only be known once code exists
    /// (`limit:frame-size`, `limit:function-size`), or a signature the
    /// target's `assign` refuses (`abi:<what>`). A fallback reason.
    Unsupported {
        /// The stable name the coverage log reports.
        construct: &'static str,
    },
    /// An [`CgError::Unsupported`] with words saying what was met (WP-189:
    /// the debug node, its kind and tag, of `debug:die-unsupported`). The
    /// coverage log reports `construct`; `detail` is the refusal's detail.
    UnsupportedDetail {
        /// The stable name.
        construct: &'static str,
        /// What was met.
        detail: String,
    },
    /// A lowering met a case `classify` should have rejected, or an
    /// invariant of the value-location policy failed: a bug in hod, or
    /// invalid IR. Never a panic and never silently wrong code.
    Internal(String),
}

/// The result of a step of code generation.
pub type CgResult<T> = Result<T, CgError>;

/// A frame larger than the allocator, or the target's prologue, can
/// express (§5.4).
pub const LIMIT_FRAME_SIZE: &str = "limit:frame-size";

/// A function whose own code a far branch cannot span (§6.3).
pub const LIMIT_FUNCTION_SIZE: &str = "limit:function-size";

/// A `musttail` call of a function that returns in memory without an
/// `sret` parameter: the buffer's address would have to be forwarded.
pub const CALL_MUSTTAIL_RET_MEMORY: &str = "call:musttail-ret-memory";

/// A part of a scalar argument or return value assigned to a register of
/// the other class: glue moves only within a class.
pub const CALL_PART_CLASS: &str = "call:part-register-class";

/// A part of kind `PartKind::Lanes` that the target does not move
/// (`Target::lanes_out`, `Target::lanes_in`), or one of a runtime call's
/// own operands (WP-082).
pub const CALL_PART_LANES: &str = "call:part-lanes";

/// A `byval` argument that `assign` does not place wholly on the stack.
pub const CALL_BYVAL_PLACE: &str = "call:byval-not-on-stack";

/// A `#dbg_assign` (§9.5).
pub const DEBUG_DBG_ASSIGN: &str = "debug:dbg-assign";

/// A `#dbg_declare` of a constant, a symbol, or a value that never got
/// a home (§4.1, §9.5).
pub const DEBUG_DECLARE_ADDRESS: &str = "debug:declare-address";

/// A declared variable in a frame place the target's frame base does
/// not reach by a fixed offset (`Target::frame_base_offset`).
pub const DEBUG_FRAME_PLACE: &str = "debug:frame-place";

impl CgError {
    /// An [`CgError::Internal`] with `text`.
    #[cold]
    pub fn internal(text: impl Into<String>) -> CgError {
        CgError::Internal(text.into())
    }

    /// An [`CgError::Unsupported`] named `construct`.
    #[cold]
    pub fn unsupported(construct: &'static str) -> CgError {
        CgError::Unsupported { construct }
    }
}

/// `Err(CgError::Internal(..))`, for a `return`.
#[cold]
pub(crate) fn internal<T>(text: impl Into<String>) -> CgResult<T> {
    Err(CgError::Internal(text.into()))
}

impl fmt::Display for CgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CgError::Unsupported { construct } => write!(f, "unsupported: {construct}"),
            CgError::UnsupportedDetail { construct, detail } => {
                write!(f, "unsupported: {construct}: {detail}")
            }
            CgError::Internal(text) => write!(f, "internal error in code generation: {text}"),
        }
    }
}

impl std::error::Error for CgError {}
