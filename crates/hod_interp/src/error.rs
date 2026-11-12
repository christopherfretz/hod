//! Why a run stopped without a result (`spec/interp.md` §5, §7).

use std::fmt;

macro_rules! traps {
    ($($(#[$meta:meta])* $variant:ident = $name:literal,)+) => {
        /// A named reason why a program has no result that every correct
        /// compilation agrees on (§5), or why a configured limit ended the
        /// run (§7).
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum Trap {
            $($(#[$meta])* $variant,)+
        }

        impl Trap {
            /// Every trap.
            pub const ALL: &'static [Trap] = &[$(Trap::$variant,)+];

            /// The stable name.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Trap::$variant => $name,)+
                }
            }
        }
    };
}

traps! {
    /// `udiv`, `sdiv`, `urem` or `srem` by zero.
    DivisionByZero = "division-by-zero",
    /// `sdiv` or `srem` of the smallest value by -1.
    DivisionOverflow = "division-overflow",
    /// A shift by the width or more.
    ShiftTooLarge = "shift-too-large",
    /// `fptoui` or `fptosi` of a value that does not fit, or of a NaN.
    FloatToIntOverflow = "float-to-int-overflow",
    /// A floating-point operation produced a NaN, whose sign and payload
    /// are not specified, and the run asked for [`crate::NanPolicy::Trap`].
    NanResult = "nan-result",
    /// An intrinsic whose result is poison for these operands
    /// (`ctlz(0, true)`, `abs(MIN, true)`).
    PoisonIntrinsic = "intrinsic-poison",
    /// An access that is not wholly inside one live data allocation.
    OutOfBounds = "out-of-bounds",
    /// An access to stack memory whose frame has returned.
    UseAfterFree = "use-after-free",
    /// An access whose alignment the allocation does not guarantee.
    Misaligned = "misaligned",
    /// A load, or a read by a host function, of a byte never written.
    UninitializedLoad = "uninitialized-load",
    /// A load of an integer whose width is not a whole number of bytes
    /// from memory whose bits above the width are not zero.
    NonZeroPadding = "non-zero-padding-bits",
    /// A store to a `constant` variable.
    WriteToConstant = "write-to-constant",
    /// `llvm.memcpy` with ranges that overlap without being equal.
    OverlappingCopy = "overlapping-copy",
    /// A use of an instruction result before the instruction ran.
    UndefinedValue = "undefined-value",
    /// `unreachable` was executed.
    Unreachable = "unreachable",
    /// `llvm.trap` was executed.
    TrapIntrinsic = "llvm.trap",
    /// A call through an address that is not a function's.
    NotAFunction = "not-a-function",
    /// A call with a function type that is not the callee's.
    FunctionTypeMismatch = "function-type-mismatch",
    /// A call with a calling convention that is not the callee's.
    CallConvMismatch = "calling-convention-mismatch",
    /// A call marked `noreturn` returned.
    NoReturnReturned = "noreturn-returned",
    /// A host function refused its arguments.
    Host = "host",
    /// `indirectbr` to an address that is not one of its destinations.
    IndirectBrTarget = "indirectbr-target",
    /// A `blockaddress` compared with anything but `null`, or converted
    /// to an integer: LLVM leaves both undefined (§5.5).
    BlockAddressInspected = "blockaddress-inspected",
    /// An exception for which the search phase finds no handler: the
    /// Itanium runtime calls `std::terminate` without unwinding (§5.9).
    UncaughtException = "uncaught-exception",
    /// An exception would leave a call or a function marked `nounwind`.
    UnwindFromNounwind = "unwind-from-nounwind",
    /// The cleanup phase of an unwind meets a handler the search phase did
    /// not choose, or does not meet the one it chose: what happens then
    /// differs between C++ runtimes (§5.9).
    EhPhaseMismatch = "eh-phase-mismatch",
    /// `resume` or `hod_catch` of something that is not an exception
    /// waiting in a landing pad.
    NotAnException = "not-an-exception",
    /// The instruction budget ran out.
    Fuel = "limit-fuel",
    /// Calls nested deeper than the configured limit.
    CallDepth = "limit-call-depth",
    /// Live memory would exceed the configured limit.
    Memory = "limit-memory",
}

impl Trap {
    /// Whether this is a configured limit (§7) rather than a property of
    /// the program.
    pub const fn is_limit(self) -> bool {
        matches!(self, Trap::Fuel | Trap::CallDepth | Trap::Memory)
    }
}

impl fmt::Display for Trap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why a run has no result.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Stop {
    /// The program has no compilation-independent result, or hit a limit.
    Trap(Trap),
    /// The program uses something the interpreter does not implement. The
    /// reason is a stable `group:name` string (§8).
    Unsupported(String),
    /// The program or the call into it is not well formed: something
    /// `Body::verify` would refuse, an argument list that does not fit the
    /// entry function, a host function that returned the wrong kind of
    /// value. The text is for people.
    Invalid(String),
}

impl Stop {
    /// The trap, if the run trapped.
    pub fn trap(&self) -> Option<Trap> {
        match self {
            Stop::Trap(trap) => Some(*trap),
            _ => None,
        }
    }

    /// The reason, if the run met something unsupported.
    pub fn unsupported(&self) -> Option<&str> {
        match self {
            Stop::Unsupported(reason) => Some(reason),
            _ => None,
        }
    }
}

impl From<Trap> for Stop {
    fn from(trap: Trap) -> Stop {
        Stop::Trap(trap)
    }
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stop::Trap(trap) => write!(f, "trap: {trap}"),
            Stop::Unsupported(reason) => write!(f, "unsupported: {reason}"),
            Stop::Invalid(what) => write!(f, "invalid: {what}"),
        }
    }
}

impl std::error::Error for Stop {}

/// `Err(Stop::Unsupported(reason))`.
pub(crate) fn unsupported<T>(reason: impl Into<String>) -> Result<T, Stop> {
    Err(Stop::Unsupported(reason.into()))
}

/// `Err(Stop::Invalid(what))`.
pub(crate) fn invalid<T>(what: impl Into<String>) -> Result<T, Stop> {
    Err(Stop::Invalid(what.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §5, §7: trap names are distinct, stable kebab-case strings, and the
    /// three limits are told apart from program traps.
    #[test]
    fn trap_names_are_distinct() {
        let mut names: Vec<&str> = Trap::ALL.iter().map(|trap| trap.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Trap::ALL.len());
        let limits: Vec<Trap> = Trap::ALL.iter().copied().filter(|trap| trap.is_limit()).collect();
        assert_eq!(limits, [Trap::Fuel, Trap::CallDepth, Trap::Memory]);
        assert_eq!(Trap::ShiftTooLarge.to_string(), "shift-too-large");
        assert_eq!(Stop::from(Trap::Fuel).to_string(), "trap: limit-fuel");
        assert_eq!(Stop::Unsupported("inst:va_arg".into()).unsupported(), Some("inst:va_arg"));
        assert_eq!(Stop::Unsupported("inst:va_arg".into()).trap(), None);
        assert_eq!(Stop::Trap(Trap::Host).trap(), Some(Trap::Host));
        assert_eq!(Stop::Invalid("x".into()).to_string(), "invalid: x");
    }
}
