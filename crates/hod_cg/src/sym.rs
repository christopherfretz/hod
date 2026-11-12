//! What code refers to: module symbols and runtime functions
//! (`spec/codegen.md` §7.3, §9.2).

use hod_ir::{SymId, TypeId};

/// The target of a relocation in a function's code (§9.2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SymRef {
    /// A symbol of the module.
    Module(SymId),
    /// A function that is not in the module (§7.3). Module emission maps
    /// it to the module's own symbol of that name if it declares one, and
    /// to a new undefined global symbol otherwise.
    Runtime(RuntimeFn),
}

macro_rules! runtime_fns {
    ($($(#[$meta:meta])* $variant:ident = $name:literal : ($($param:ident),*) -> $ret:ident;)+) => {
        /// The functions outside the module that lowerings call (§7.3): a
        /// closed list. A member is added together with the lowering that
        /// calls it.
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum RuntimeFn {
            $($(#[$meta])* $variant,)+
        }

        impl RuntimeFn {
            /// Every member.
            pub const ALL: &'static [RuntimeFn] = &[$(RuntimeFn::$variant,)+];

            /// The symbol name.
            pub const fn name(self) -> &'static str {
                match self {
                    $(RuntimeFn::$variant => $name,)+
                }
            }

            /// The parameter types. All are types with fixed ids, so the
            /// list needs no type table.
            pub const fn params(self) -> &'static [TypeId] {
                match self {
                    $(RuntimeFn::$variant => &[$(TypeId::$param,)*],)+
                }
            }

            /// The return type; `TypeId::VOID` for none.
            pub const fn ret(self) -> TypeId {
                match self {
                    $(RuntimeFn::$variant => TypeId::$ret,)+
                }
            }
        }
    };
}

runtime_fns! {
    /// `void *memcpy(void *, const void *, size_t)`.
    Memcpy = "memcpy": (PTR, PTR, I64) -> PTR;
    /// `void *memmove(void *, const void *, size_t)`.
    Memmove = "memmove": (PTR, PTR, I64) -> PTR;
    /// `void *memset(void *, int, size_t)`.
    Memset = "memset": (PTR, I32, I64) -> PTR;
    /// Unsigned 128-bit division.
    UDivTi3 = "__udivti3": (I128, I128) -> I128;
    /// Signed 128-bit division.
    DivTi3 = "__divti3": (I128, I128) -> I128;
    /// Unsigned 128-bit remainder.
    UModTi3 = "__umodti3": (I128, I128) -> I128;
    /// Signed 128-bit remainder.
    ModTi3 = "__modti3": (I128, I128) -> I128;
    /// `fp128` addition.
    AddTf3 = "__addtf3": (FP128, FP128) -> FP128;
    /// `fp128` subtraction.
    SubTf3 = "__subtf3": (FP128, FP128) -> FP128;
    /// `fp128` multiplication.
    MulTf3 = "__multf3": (FP128, FP128) -> FP128;
    /// `fp128` division.
    DivTf3 = "__divtf3": (FP128, FP128) -> FP128;
    /// `double fmod(double, double)`: `frem`.
    Fmod = "fmod": (DOUBLE, DOUBLE) -> DOUBLE;
    /// `float fmodf(float, float)`: `frem`.
    Fmodf = "fmodf": (FLOAT, FLOAT) -> FLOAT;
    /// `void _Unwind_Resume(struct _Unwind_Exception *)`: `resume`.
    UnwindResume = "_Unwind_Resume": (PTR) -> VOID;
    /// `void *__tls_get_addr(tls_index *)`: the general-dynamic TLS helper.
    TlsGetAddr = "__tls_get_addr": (PTR) -> PTR;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §7.3: the list is closed and its names are distinct symbol names.
    #[test]
    fn runtime_functions_have_distinct_names_and_fixed_types() {
        for (index, &f) in RuntimeFn::ALL.iter().enumerate() {
            assert!(!f.name().is_empty());
            assert!(RuntimeFn::ALL[..index].iter().all(|other| other.name() != f.name()));
            assert!(f.params().iter().all(|&ty| ty != TypeId::VOID));
        }
        assert_eq!(RuntimeFn::Memcpy.params(), [TypeId::PTR, TypeId::PTR, TypeId::I64]);
        assert_eq!(RuntimeFn::UnwindResume.ret(), TypeId::VOID);
        assert_eq!(RuntimeFn::UDivTi3.ret(), TypeId::I128);
    }
}
