//! The module shell (`spec/ir.md` §2): the owner of the tables.
//!
//! WP-002 provides the target, the type table and the constant pool. The
//! symbol, COMDAT, inline-assembly and debug tables are added to this
//! struct by the work packages that own them.

use crate::constant::ConstPool;
use crate::target::TargetSpec;
use crate::types::TypeTable;

/// One translation unit's IR.
///
/// The tables are public fields so that a caller can borrow them
/// separately: constant constructors take the type table by reference
/// (`module.consts.int(&module.types, ty, value)`). They are append-only:
/// an id, once returned, stays valid and its entry never changes.
#[derive(Clone, Debug)]
pub struct Module {
    target: TargetSpec,
    /// The type table (§3), laid out for the target's data layout.
    pub types: TypeTable,
    /// The constant pool (§4).
    pub consts: ConstPool,
}

impl Module {
    /// An empty module for `target`.
    pub fn new(target: TargetSpec) -> Module {
        let types = TypeTable::new(target.data_layout());
        Module { target, types, consts: ConstPool::new() }
    }

    /// What the module is compiled for.
    pub fn target(&self) -> &TargetSpec {
        &self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::TypeId;

    /// §2.1: a module's type table is laid out for its target.
    #[test]
    fn module_tables_follow_the_target() {
        let a64 = Module::new(TargetSpec::aarch64_linux_gnu());
        let x64 = Module::new(TargetSpec::x86_64_linux_gnu());
        assert_eq!(a64.target().triple(), "aarch64-unknown-linux-gnu");
        assert_eq!(a64.types.data_layout(), a64.target().data_layout());
        assert_eq!(a64.types.pref_align_of(TypeId::I8), 4);
        assert_eq!(x64.types.pref_align_of(TypeId::I8), 1);
        assert_eq!(a64.consts.len(), 0);
    }
}
