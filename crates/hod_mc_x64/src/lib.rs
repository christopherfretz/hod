//! x86-64 instruction encoder: typed constructors that turn an instruction
//! and its operands into its bytes.
//!
//! Contract: `spec/mc-x64.md`. Role in the pipeline: `docs/ARCHITECTURE.md`.
//!
//! The crate knows nothing about the IR, object files or assembly text. Its
//! consumer is a single-pass code generator, so every constructor is a plain
//! `const fn` returning a small `Copy` value, [`Inst`]: up to 15 bytes, the
//! length, and where the relocatable field is. No allocation, no traits, no
//! builder state.
//!
//! ```
//! use hod_mc_x64::{Mem, Reg, Size, add_ri, mov_rm, try_add_ri};
//!
//! // add rax, 16
//! assert_eq!(add_ri(Size::Q, Reg::Rax, 16).bytes(), [0x48, 0x83, 0xc0, 0x10]);
//! // mov ecx, dword ptr [rsp + 8]
//! let load = mov_rm(Size::D, Reg::Rcx, Mem::base(Reg::Rsp, 8));
//! assert_eq!(load.bytes(), [0x8b, 0x4c, 0x24, 0x08]);
//! // A 64-bit ADD takes a sign-extended 32-bit immediate: 2^31 is not one.
//! assert_eq!(try_add_ri(Size::Q, Reg::Rax, 1 << 31), None);
//! ```
//!
//! # Registers and sizes
//!
//! [`Reg`] is the register number, `rax`..`r15`, and [`Xmm`] that of an
//! SSE register, `xmm0`..`xmm15`; the operand size is a separate operand,
//! [`Size`] (`B`, `W`, `D`, `Q`) or, for floating point, [`FpSize`] (`S`,
//! `D`), so `Reg::Rsi` with `Size::B` is `sil` and gets the REX prefix
//! that needs. The high-byte
//! registers `ah`..`bh` are not provided. A `SIB` index is an [`Index`], which
//! cannot be `rsp`; a memory operand is a [`Mem`], every value of which has
//! an encoding.
//!
//! # Checked and unchecked forms
//!
//! A constructor whose operands are all valid by type is infallible. A
//! constructor `f` with an operand precondition (an immediate that may not
//! fit, a size the instruction does not exist in) has a twin `try_f` that
//! returns `None` exactly when the precondition does not hold. `f` itself
//! checks the precondition with `debug_assert!`; with debug assertions off,
//! violating it yields unspecified bytes.

/// Defines an unchecked constructor and its checked `try_` twin from one
/// statement of the precondition.
macro_rules! checked {
    ($(
        $(#[$meta:meta])*
        pub fn $name:ident / $try_name:ident ( $($arg:ident : $ty:ty),* $(,)? )
        requires $pre:expr;
        $body:block
    )+) => {$(
        $(#[$meta])*
        ///
        /// The precondition is checked by `debug_assert!`; the `try_` twin
        /// returns `None` instead.
        #[inline]
        pub const fn $name($($arg: $ty),*) -> $crate::Inst {
            debug_assert!($pre, concat!(stringify!($name), ": operand precondition violated"));
            $body
        }

        #[doc = concat!(
            "Checked form of [`", stringify!($name),
            "`]: `None` exactly when its precondition does not hold."
        )]
        #[inline]
        pub const fn $try_name($($arg: $ty),*) -> Option<$crate::Inst> {
            if $pre { Some($name($($arg),*)) } else { None }
        }
    )+};
}

/// Defines constructors that share a signature and an encoding helper.
/// Each entry names the leading arguments of the helper (the fields that
/// select the instruction within its class); the constructor's own operands
/// follow them.
///
/// With a `requires` clause after the signature every entry is a
/// [`checked!`] pair, written `pub fn name / try_name = (..);`.
macro_rules! family {
    (@plain ($($arg:ident : $ty:ty),*) $helper:ident [$(#[$meta:meta])*] $name:ident
     ($($op:expr),+)) => {
        $(#[$meta])*
        #[inline]
        pub const fn $name($($arg: $ty),*) -> $crate::Inst {
            $helper($($op,)+ $($arg),*)
        }
    };
    (@checked ($($arg:ident : $ty:ty),*) ($pre:expr) $helper:ident [$(#[$meta:meta])*]
     $name:ident $try_name:ident ($($op:expr),+)) => {
        checked! {
            $(#[$meta])*
            pub fn $name / $try_name($($arg: $ty),*)
            requires $pre;
            { $helper($($op,)+ $($arg),*) }
        }
    };
    (
        $sig:tt => $helper:ident;
        $(
            $(#[$meta:meta])*
            pub fn $name:ident = ($($op:expr),+);
        )+
    ) => {$(
        family!(@plain $sig $helper [$(#[$meta])*] $name ($($op),+));
    )+};
    (
        $sig:tt requires $pre:tt => $helper:ident;
        $(
            $(#[$meta:meta])*
            pub fn $name:ident / $try_name:ident = ($($op:expr),+);
        )+
    ) => {$(
        family!(@checked $sig $pre $helper [$(#[$meta])*] $name $try_name ($($op),+));
    )+};
}

pub mod alu;
pub mod atomic;
pub mod bits;
pub mod branch;
pub mod bt;
pub mod cond;
pub mod fixup;
pub mod inst;
pub mod mov;
pub mod packed;
pub mod reg;
pub mod shift;
pub mod sse;
pub mod state;
pub mod system;
pub mod tls;

pub use alu::*;
pub use atomic::*;
pub use bits::*;
pub use branch::*;
pub use bt::*;
pub use cond::*;
pub use fixup::Fixup;
pub use inst::{Inst, MAX_LEN, Site};
pub use mov::*;
pub use packed::*;
pub use reg::{Base, Cond, FpCmp, FpSize, Index, Mem, Reg, Scale, Seg, Size, Xmm};
pub use shift::*;
pub use sse::*;
pub use state::*;
pub use system::*;
pub use tls::*;

// x87 (WP-119).
pub mod x87;
pub use x87::*;
