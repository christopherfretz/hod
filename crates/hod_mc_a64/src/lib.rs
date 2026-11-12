//! AArch64 instruction encoder: typed constructors that turn an instruction
//! and its operands into the 32-bit instruction word.
//!
//! Contract: `spec/mc-a64.md`. Role in the pipeline: `docs/ARCHITECTURE.md`.
//!
//! The crate knows nothing about the IR, object files or assembly text. Its
//! consumer is a single-pass code generator, so every constructor is a plain
//! `const fn` returning `u32`: no allocation, no traits, no builder state.
//!
//! ```
//! use hod_mc_a64::{Reg, RegSp, Size, add_imm, try_add_imm};
//!
//! // add x0, sp, #16
//! assert_eq!(add_imm(Size::X, Reg::X0.sp(), RegSp::SP, 16), 0x9100_43e0);
//! // 0x1001 is not an ADD immediate: materialize it instead.
//! assert_eq!(try_add_imm(Size::X, Reg::X0.sp(), RegSp::SP, 0x1001), None);
//! ```
//!
//! # Registers
//!
//! Encoding 31 means the stack pointer in some operand positions and the
//! zero register in others. The API makes the caller say which: [`Reg`] is
//! `X0`..`X30`, [`RegSp`] is a register or SP, [`RegZr`] is a register or
//! ZR, and every constructor's signature states which one each position
//! takes.
//!
//! The FP/SIMD registers are [`VReg`], `V0`..`V31`. Which scalar view (`B`,
//! `H`, `S`, `D`, `Q`) or vector arrangement (`8B`..`2D`) an instruction
//! uses is an operand of the constructor ([`FpSize`], [`Arr`], ...), like
//! the size of a general-purpose register.
//!
//! ```
//! use hod_mc_a64::{FpFormat, FpSize, Reg, VReg, fadd, fmov_const};
//!
//! // fadd d0, d1, d2
//! assert_eq!(fadd(FpSize::D, VReg::V0, VReg::V1, VReg::V2), 0x1e62_2820);
//! // 1.0 is an FMOV immediate; 0.1 goes through a scratch register.
//! assert_eq!(fmov_const(FpFormat::D, VReg::V0, Reg::X9, 1.0f64.to_bits()).len(), 1);
//! assert_eq!(fmov_const(FpFormat::D, VReg::V0, Reg::X9, 0.1f64.to_bits()).len(), 4);
//! ```
//!
//! # Checked and unchecked forms
//!
//! A constructor whose operands are all valid by type is infallible. A
//! constructor `f` with an operand precondition (an immediate that may not
//! fit, a shift amount, a register constraint) has a twin `try_f` that
//! returns `None` exactly when the precondition does not hold. `f` itself
//! checks the precondition with `debug_assert!`; with debug assertions off,
//! violating it yields an unspecified instruction word.

#![allow(
    clippy::similar_names,
    reason = "operand names follow the Arm ARM: rn/rm, rt/rt2, immr/imms"
)]

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
        pub const fn $name($($arg: $ty),*) -> u32 {
            debug_assert!($pre, concat!(stringify!($name), ": operand precondition violated"));
            $body
        }

        #[doc = concat!(
            "Checked form of [`", stringify!($name),
            "`]: `None` exactly when its precondition does not hold."
        )]
        #[inline]
        pub const fn $try_name($($arg: $ty),*) -> Option<u32> {
            if $pre { Some($name($($arg),*)) } else { None }
        }
    )+};
}

/// Defines constructors that share a signature and an encoding-class
/// helper. Each entry names the leading arguments of the helper (the fields
/// that select the instruction within its class); the constructor's own
/// operands follow them.
///
/// With a `requires` clause after the signature every entry is a
/// [`checked!`] pair, written `pub fn name / try_name = (..);`.
macro_rules! family {
    (@plain ($($arg:ident : $ty:ty),*) $helper:ident [$(#[$meta:meta])*] $name:ident
     ($($op:expr),+)) => {
        $(#[$meta])*
        #[inline]
        pub const fn $name($($arg: $ty),*) -> u32 {
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

pub mod arith;
pub mod atomic;
pub mod bitfield;
pub mod branch;
pub mod condsel;
pub mod fixup;
pub mod fp;
pub mod fpmem;
pub mod imm;
pub mod logical;
pub mod mem;
pub mod neon;
pub mod reg;
pub mod simd;
pub mod system;
pub mod wide;

pub use arith::*;
pub use atomic::*;
pub use bitfield::*;
pub use branch::*;
pub use condsel::*;
pub use fixup::Fixup;
pub use fp::*;
pub use fpmem::*;
pub use imm::{decode_bitmask, decode_fp_imm8, encode_bitmask, encode_fp_imm8, is_arith_imm};
pub use logical::*;
pub use mem::*;
pub use neon::*;
pub use reg::{
    Arr, Cond, Elem, Extend, FpArr, FpFormat, FpSize, Reg, RegSp, RegZr, Shift, Size, VReg, VecBits,
};
pub use simd::*;
pub use system::*;
pub use wide::*;
