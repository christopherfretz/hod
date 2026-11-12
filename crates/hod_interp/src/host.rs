//! The standard host functions (`spec/interp.md` §6.1).
//!
//! Two functions a test program can observe the world through and be
//! observed through: `void sink(i64)`, whose calls are the program's
//! output, and `i64 source()`, a fixed sequence of inputs.

use hod_ir::{FunctionDesc, IrResult, Module, SymId, TypeId};

use crate::error::Trap;
use crate::interp::Interp;
use crate::value::Val;

/// The symbol name of `void sink(i64)`.
pub const SINK: &str = "sink";

/// The symbol name of `i64 source()`.
pub const SOURCE: &str = "source";

/// The symbols of the standard host functions in one module.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Standard {
    /// `void sink(i64)`.
    pub sink: SymId,
    /// The function type of `sink`.
    pub sink_ty: TypeId,
    /// `i64 source()`.
    pub source: SymId,
    /// The function type of `source`.
    pub source_ty: TypeId,
}

/// Declares the standard host functions in `module`: external functions
/// without bodies.
///
/// # Errors
/// What `hod_ir` refuses: a symbol of one of the names that is already
/// something else.
pub fn declare(module: &mut Module) -> IrResult<Standard> {
    let sink_ty = module.types.function(TypeId::VOID, &[TypeId::I64], false)?;
    let source_ty = module.types.function(TypeId::I64, &[], false)?;
    let sink = module.add_function(SINK.as_bytes(), &FunctionDesc::new(sink_ty))?;
    let source = module.add_function(SOURCE.as_bytes(), &FunctionDesc::new(source_ty))?;
    Ok(Standard { sink, sink_ty, source, source_ty })
}

/// What the call of `source` after `index` completed ones returns: the
/// `index`-th output of `SplitMix64` seeded with zero.
pub const fn source_value(index: u64) -> u64 {
    let mut z = index.wrapping_add(1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Registers the standard host functions with `interp`.
pub fn register(interp: &mut Interp<'_>) {
    interp.host(SINK, |_, args| match args {
        [Val::Int { bits: 64, .. }] => Ok(None),
        _ => Err(Trap::Host),
    });
    interp.host(SOURCE, |cx, args| match args {
        [] => Ok(Some(Val::i64(source_value(cx.index)))),
        _ => Err(Trap::Host),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §6.1: the sequence `source` returns is `SplitMix64` from seed zero.
    #[test]
    fn source_is_splitmix64() {
        // The reference implementation's first outputs for seed 0.
        assert_eq!(source_value(0), 0xe220_a839_7b1d_cdaf);
        assert_eq!(source_value(1), 0x6e78_9e6a_a1b9_65f4);
        assert_eq!(source_value(2), 0x06c4_5d18_8009_454f);
    }
}
