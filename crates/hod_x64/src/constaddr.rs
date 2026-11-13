//! Constant operands that hold addresses beyond a symbol plus an offset:
//! differences of symbols and blocks, `Expr` constants, images with
//! relocations: WP-126. `common::materialize_gpr_const` refuses these
//! as [`CONSTADDR`] until then.

/// The group's refusal until WP-126.
pub const CONSTADDR: &str = "x64:todo:constaddr";
