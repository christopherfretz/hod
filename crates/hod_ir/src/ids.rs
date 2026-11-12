//! Table ids (`spec/ir.md` §1): `u32` newtypes, one per table.
//!
//! An id is an index into its module's table. It is meaningful only for the
//! module that returned it; using it with another module panics or names an
//! unrelated entry.

use std::fmt;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u32);

        impl $name {
            /// The id with raw value `raw`. For the code that owns the table.
            #[inline]
            pub const fn new(raw: u32) -> Self {
                Self(raw)
            }

            /// The raw value.
            #[inline]
            pub const fn raw(self) -> u32 {
                self.0
            }

            /// The id as a table index.
            #[inline]
            pub const fn index(self) -> usize {
                self.0 as usize
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }
    };
}

id_type!(
    /// A type in a module's type table (§3). Equal types have equal ids.
    TypeId,
    "ty"
);
id_type!(
    /// A constant in a module's constant pool (§4). Equal constants have
    /// equal ids.
    ConstId,
    "c"
);
id_type!(
    /// A symbol: a function, variable, alias or IFUNC (§2.2). Constants
    /// refer to symbols by id and need no symbol table to do so.
    SymId,
    "sym"
);
id_type!(
    /// A basic block of a function body (§5.3). Appears in constants only in
    /// [`crate::Const::BlockAddr`].
    BlockId,
    "bb"
);

id_type!(
    /// A COMDAT of a module (§2): a name and a selection kind.
    ComdatId,
    "comdat"
);
id_type!(
    /// An inline-assembly callee of a module (§2): template, constraints,
    /// flags and function type.
    AsmId,
    "asm"
);
id_type!(
    /// A debug-information node of a module (§6).
    DiId,
    "di"
);
id_type!(
    /// An interned source location of a module (§6): line, column, scope
    /// and inlined-at.
    LocId,
    "loc"
);

impl LocId {
    /// "No location": what an instruction without `!dbg` has.
    pub const NONE: LocId = LocId(u32::MAX);
}

impl TypeId {
    /// `void`.
    pub const VOID: TypeId = TypeId(0);
    /// `label`.
    pub const LABEL: TypeId = TypeId(1);
    /// `metadata`.
    pub const METADATA: TypeId = TypeId(2);
    /// `token`.
    pub const TOKEN: TypeId = TypeId(3);
    /// `half` (IEEE binary16).
    pub const HALF: TypeId = TypeId(4);
    /// `bfloat`.
    pub const BFLOAT: TypeId = TypeId(5);
    /// `float` (IEEE binary32).
    pub const FLOAT: TypeId = TypeId(6);
    /// `double` (IEEE binary64).
    pub const DOUBLE: TypeId = TypeId(7);
    /// `fp128` (IEEE binary128).
    pub const FP128: TypeId = TypeId(8);
    /// `x86_fp80`.
    pub const X86_FP80: TypeId = TypeId(9);
    /// `ptr` in address space 0.
    pub const PTR: TypeId = TypeId(10);
    /// `i1`.
    pub const I1: TypeId = TypeId(11);
    /// `i8`.
    pub const I8: TypeId = TypeId(12);
    /// `i16`.
    pub const I16: TypeId = TypeId(13);
    /// `i32`.
    pub const I32: TypeId = TypeId(14);
    /// `i64`.
    pub const I64: TypeId = TypeId(15);
    /// `i128`.
    pub const I128: TypeId = TypeId(16);
    /// Number of types with fixed ids.
    pub(crate) const FIXED_COUNT: u32 = 17;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §1: ids are four bytes and print as a prefix and the index.
    #[test]
    fn ids_are_u32_and_print_with_prefix() {
        assert_eq!(size_of::<TypeId>(), 4);
        assert_eq!(size_of::<ConstId>(), 4);
        assert_eq!(size_of::<SymId>(), 4);
        assert_eq!(size_of::<BlockId>(), 4);
        assert_eq!(size_of::<ComdatId>(), 4);
        assert_eq!(size_of::<AsmId>(), 4);
        assert_eq!(size_of::<DiId>(), 4);
        assert_eq!(size_of::<LocId>(), 4);
        assert_eq!(format!("{:?}", ComdatId::new(1)), "comdat1");
        assert_eq!(format!("{:?}", AsmId::new(2)), "asm2");
        assert_eq!(format!("{:?}", DiId::new(3)), "di3");
        assert_eq!(format!("{:?}", LocId::new(4)), "loc4");
        assert_ne!(LocId::NONE, LocId::new(0));
        assert_eq!(size_of::<Option<TypeId>>(), 8);
        assert_eq!(format!("{:?}", TypeId::new(12)), "ty12");
        assert_eq!(format!("{:?}", ConstId::new(0)), "c0");
        assert_eq!(format!("{:?}", SymId::new(7)), "sym7");
        assert_eq!(format!("{:?}", BlockId::new(3)), "bb3");
        assert_eq!(TypeId::new(9).index(), 9);
        assert_eq!(TypeId::new(9).raw(), 9);
        assert!(TypeId::I8 < TypeId::I16);
    }
}
