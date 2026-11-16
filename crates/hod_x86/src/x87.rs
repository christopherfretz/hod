//! Floating point on the x87 stack: design 0008's I9 owns this file.
//! WP-293 refuses every floating-point value under [`FLOAT`].

/// A floating-point value, `fcmp`, `fneg`, a floating-point cast or
/// intrinsic: removed by I9.
pub const FLOAT: &str = "x86:float";
