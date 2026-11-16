//! Integers wider than 32 bits: design 0008's I7 owns this file. WP-293
//! refuses them all under [`I64`].

/// A value of an integer type wider than 32 bits (`i33`..`i64`, `i128`,
/// any width): removed by I7.
pub const I64: &str = "x86:i64";
