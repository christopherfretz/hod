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
    /// `fp128` equality: 0 when equal and ordered (WP-067).
    EqTf2 = "__eqtf2": (FP128, FP128) -> I32;
    /// `fp128` less than: negative when less and ordered, positive when
    /// unordered (WP-067).
    LtTf2 = "__lttf2": (FP128, FP128) -> I32;
    /// `fp128` less or equal: at most 0 when so and ordered, positive
    /// when unordered (WP-067).
    LeTf2 = "__letf2": (FP128, FP128) -> I32;
    /// `fp128` unordered: non-zero when either is a NaN (WP-067).
    UnordTf2 = "__unordtf2": (FP128, FP128) -> I32;
    /// `fpext float` to `fp128` (WP-067).
    ExtendSfTf2 = "__extendsftf2": (FLOAT) -> FP128;
    /// `fpext double` to `fp128` (WP-067).
    ExtendDfTf2 = "__extenddftf2": (DOUBLE) -> FP128;
    /// `fptrunc fp128` to `half` (WP-067).
    TruncTfHf2 = "__trunctfhf2": (FP128) -> HALF;
    /// `fptrunc fp128` to `float` (WP-067).
    TruncTfSf2 = "__trunctfsf2": (FP128) -> FLOAT;
    /// `fptrunc fp128` to `double` (WP-067).
    TruncTfDf2 = "__trunctfdf2": (FP128) -> DOUBLE;
    /// `fptosi fp128` to at most 32 bits (WP-067).
    FixTfSi = "__fixtfsi": (FP128) -> I32;
    /// `fptosi fp128` to at most 64 bits (WP-067).
    FixTfDi = "__fixtfdi": (FP128) -> I64;
    /// `fptoui fp128` to at most 32 bits (WP-067).
    FixunsTfSi = "__fixunstfsi": (FP128) -> I32;
    /// `fptoui fp128` to at most 64 bits (WP-067).
    FixunsTfDi = "__fixunstfdi": (FP128) -> I64;
    /// `sitofp` of at most 32 bits to `fp128` (WP-067).
    FloatSiTf = "__floatsitf": (I32) -> FP128;
    /// `sitofp` of at most 64 bits to `fp128` (WP-067).
    FloatDiTf = "__floatditf": (I64) -> FP128;
    /// `uitofp` of at most 32 bits to `fp128` (WP-067).
    FloatunSiTf = "__floatunsitf": (I32) -> FP128;
    /// `uitofp` of at most 64 bits to `fp128` (WP-067).
    FloatunDiTf = "__floatunditf": (I64) -> FP128;
    /// `long double fmodl(long double, long double)`: `frem fp128`
    /// (WP-067).
    Fmodl = "fmodl": (FP128, FP128) -> FP128;
    /// `llvm.sqrt.f128` (WP-070).
    Sqrtl = "sqrtl": (FP128) -> FP128;
    /// `llvm.floor.f128` (WP-070).
    Floorl = "floorl": (FP128) -> FP128;
    /// `llvm.ceil.f128` (WP-070).
    Ceill = "ceill": (FP128) -> FP128;
    /// `llvm.trunc.f128` (WP-070).
    Truncl = "truncl": (FP128) -> FP128;
    /// `llvm.round.f128` (WP-070).
    Roundl = "roundl": (FP128) -> FP128;
    /// `llvm.roundeven.f128` (WP-070).
    Roundevenl = "roundevenl": (FP128) -> FP128;
    /// `llvm.nearbyint.f128` (WP-070).
    Nearbyintl = "nearbyintl": (FP128) -> FP128;
    /// `llvm.rint.f128` (WP-070).
    Rintl = "rintl": (FP128) -> FP128;
    /// `llvm.fma.f128` (WP-070).
    Fmal = "fmal": (FP128, FP128, FP128) -> FP128;
    /// `llvm.minnum.f128` (WP-070).
    Fminl = "fminl": (FP128, FP128) -> FP128;
    /// `llvm.maxnum.f128` (WP-070).
    Fmaxl = "fmaxl": (FP128, FP128) -> FP128;
    /// `llvm.minimum.f128` (C23, glibc 2.35) (WP-070).
    Fminimuml = "fminimuml": (FP128, FP128) -> FP128;
    /// `llvm.maximum.f128` (C23, glibc 2.35) (WP-070).
    Fmaximuml = "fmaximuml": (FP128, FP128) -> FP128;
    /// `llvm.minimumnum.f128` (C23, glibc 2.35) (WP-070).
    FminimumNuml = "fminimum_numl": (FP128, FP128) -> FP128;
    /// `llvm.maximumnum.f128` (C23, glibc 2.35) (WP-070).
    FmaximumNuml = "fmaximum_numl": (FP128, FP128) -> FP128;
    /// `llvm.lround.*.f128` (WP-070).
    Lroundl = "lroundl": (FP128) -> I64;
    /// `llvm.llround.*.f128` (WP-070).
    Llroundl = "llroundl": (FP128) -> I64;
    /// `llvm.lrint.*.f128` (WP-070).
    Lrintl = "lrintl": (FP128) -> I64;
    /// `llvm.llrint.*.f128` (WP-070).
    Llrintl = "llrintl": (FP128) -> I64;
    /// `llvm.powi.f32` (and `half` through it) (WP-070).
    PowiSf2 = "__powisf2": (FLOAT, I32) -> FLOAT;
    /// `llvm.powi.f64` (WP-070).
    PowiDf2 = "__powidf2": (DOUBLE, I32) -> DOUBLE;
    /// `llvm.powi.f128` (WP-070).
    PowiTf2 = "__powitf2": (FP128, I32) -> FP128;
    /// `llvm.ldexp.f32` (and `half` through it) (WP-070).
    Ldexpf = "ldexpf": (FLOAT, I32) -> FLOAT;
    /// `llvm.ldexp.f64` (WP-070).
    Ldexp = "ldexp": (DOUBLE, I32) -> DOUBLE;
    /// `llvm.ldexp.f128` (WP-070).
    Ldexpl = "ldexpl": (FP128, I32) -> FP128;
    /// `llvm.frexp.f32` (and `half` through it) (WP-070).
    Frexpf = "frexpf": (FLOAT, PTR) -> FLOAT;
    /// `llvm.frexp.f64` (WP-070).
    Frexp = "frexp": (DOUBLE, PTR) -> DOUBLE;
    /// `llvm.frexp.f128` (WP-070).
    Frexpl = "frexpl": (FP128, PTR) -> FP128;
    /// `llvm.sin.f32` (and `half` through it) (WP-070).
    Sinf = "sinf": (FLOAT) -> FLOAT;
    /// `llvm.sin.f64` (WP-070).
    Sin = "sin": (DOUBLE) -> DOUBLE;
    /// `llvm.sin.f128` (WP-070).
    Sinl = "sinl": (FP128) -> FP128;
    /// `llvm.cos.f32` (and `half` through it) (WP-070).
    Cosf = "cosf": (FLOAT) -> FLOAT;
    /// `llvm.cos.f64` (WP-070).
    Cos = "cos": (DOUBLE) -> DOUBLE;
    /// `llvm.cos.f128` (WP-070).
    Cosl = "cosl": (FP128) -> FP128;
    /// `llvm.tan.f32` (and `half` through it) (WP-070).
    Tanf = "tanf": (FLOAT) -> FLOAT;
    /// `llvm.tan.f64` (WP-070).
    Tan = "tan": (DOUBLE) -> DOUBLE;
    /// `llvm.tan.f128` (WP-070).
    Tanl = "tanl": (FP128) -> FP128;
    /// `llvm.asin.f32` (and `half` through it) (WP-070).
    Asinf = "asinf": (FLOAT) -> FLOAT;
    /// `llvm.asin.f64` (WP-070).
    Asin = "asin": (DOUBLE) -> DOUBLE;
    /// `llvm.asin.f128` (WP-070).
    Asinl = "asinl": (FP128) -> FP128;
    /// `llvm.acos.f32` (and `half` through it) (WP-070).
    Acosf = "acosf": (FLOAT) -> FLOAT;
    /// `llvm.acos.f64` (WP-070).
    Acos = "acos": (DOUBLE) -> DOUBLE;
    /// `llvm.acos.f128` (WP-070).
    Acosl = "acosl": (FP128) -> FP128;
    /// `llvm.atan.f32` (and `half` through it) (WP-070).
    Atanf = "atanf": (FLOAT) -> FLOAT;
    /// `llvm.atan.f64` (WP-070).
    Atan = "atan": (DOUBLE) -> DOUBLE;
    /// `llvm.atan.f128` (WP-070).
    Atanl = "atanl": (FP128) -> FP128;
    /// `llvm.sinh.f32` (and `half` through it) (WP-070).
    Sinhf = "sinhf": (FLOAT) -> FLOAT;
    /// `llvm.sinh.f64` (WP-070).
    Sinh = "sinh": (DOUBLE) -> DOUBLE;
    /// `llvm.sinh.f128` (WP-070).
    Sinhl = "sinhl": (FP128) -> FP128;
    /// `llvm.cosh.f32` (and `half` through it) (WP-070).
    Coshf = "coshf": (FLOAT) -> FLOAT;
    /// `llvm.cosh.f64` (WP-070).
    Cosh = "cosh": (DOUBLE) -> DOUBLE;
    /// `llvm.cosh.f128` (WP-070).
    Coshl = "coshl": (FP128) -> FP128;
    /// `llvm.tanh.f32` (and `half` through it) (WP-070).
    Tanhf = "tanhf": (FLOAT) -> FLOAT;
    /// `llvm.tanh.f64` (WP-070).
    Tanh = "tanh": (DOUBLE) -> DOUBLE;
    /// `llvm.tanh.f128` (WP-070).
    Tanhl = "tanhl": (FP128) -> FP128;
    /// `llvm.exp.f32` (and `half` through it) (WP-070).
    Expf = "expf": (FLOAT) -> FLOAT;
    /// `llvm.exp.f64` (WP-070).
    Exp = "exp": (DOUBLE) -> DOUBLE;
    /// `llvm.exp.f128` (WP-070).
    Expl = "expl": (FP128) -> FP128;
    /// `llvm.exp2.f32` (and `half` through it) (WP-070).
    Exp2f = "exp2f": (FLOAT) -> FLOAT;
    /// `llvm.exp2.f64` (WP-070).
    Exp2 = "exp2": (DOUBLE) -> DOUBLE;
    /// `llvm.exp2.f128` (WP-070).
    Exp2l = "exp2l": (FP128) -> FP128;
    /// `llvm.exp10.f32` (and `half` through it) (WP-070).
    Exp10f = "exp10f": (FLOAT) -> FLOAT;
    /// `llvm.exp10.f64` (WP-070).
    Exp10 = "exp10": (DOUBLE) -> DOUBLE;
    /// `llvm.exp10.f128` (WP-070).
    Exp10l = "exp10l": (FP128) -> FP128;
    /// `llvm.log.f32` (and `half` through it) (WP-070).
    Logf = "logf": (FLOAT) -> FLOAT;
    /// `llvm.log.f64` (WP-070).
    Log = "log": (DOUBLE) -> DOUBLE;
    /// `llvm.log.f128` (WP-070).
    Logl = "logl": (FP128) -> FP128;
    /// `llvm.log2.f32` (and `half` through it) (WP-070).
    Log2f = "log2f": (FLOAT) -> FLOAT;
    /// `llvm.log2.f64` (WP-070).
    Log2 = "log2": (DOUBLE) -> DOUBLE;
    /// `llvm.log2.f128` (WP-070).
    Log2l = "log2l": (FP128) -> FP128;
    /// `llvm.log10.f32` (and `half` through it) (WP-070).
    Log10f = "log10f": (FLOAT) -> FLOAT;
    /// `llvm.log10.f64` (WP-070).
    Log10 = "log10": (DOUBLE) -> DOUBLE;
    /// `llvm.log10.f128` (WP-070).
    Log10l = "log10l": (FP128) -> FP128;
    /// `llvm.atan2.f32` (and `half` through it) (WP-070).
    Atan2f = "atan2f": (FLOAT, FLOAT) -> FLOAT;
    /// `llvm.atan2.f64` (WP-070).
    Atan2 = "atan2": (DOUBLE, DOUBLE) -> DOUBLE;
    /// `llvm.atan2.f128` (WP-070).
    Atan2l = "atan2l": (FP128, FP128) -> FP128;
    /// `llvm.pow.f32` (and `half` through it) (WP-070).
    Powf = "powf": (FLOAT, FLOAT) -> FLOAT;
    /// `llvm.pow.f64` (WP-070).
    Pow = "pow": (DOUBLE, DOUBLE) -> DOUBLE;
    /// `llvm.pow.f128` (WP-070).
    Powl = "powl": (FP128, FP128) -> FP128;
    /// `llvm.clear_cache` (WP-070).
    ClearCache = "__clear_cache": (PTR, PTR) -> VOID;
    /// `__aarch64_cas1_relax`: outline atomics (WP-074).
    Cas1Relax = "__aarch64_cas1_relax": (I8, I8, PTR) -> I8;
    /// `__aarch64_cas1_acq`: outline atomics (WP-074).
    Cas1Acq = "__aarch64_cas1_acq": (I8, I8, PTR) -> I8;
    /// `__aarch64_cas1_rel`: outline atomics (WP-074).
    Cas1Rel = "__aarch64_cas1_rel": (I8, I8, PTR) -> I8;
    /// `__aarch64_cas1_acq_rel`: outline atomics (WP-074).
    Cas1AcqRel = "__aarch64_cas1_acq_rel": (I8, I8, PTR) -> I8;
    /// `__aarch64_cas2_relax`: outline atomics (WP-074).
    Cas2Relax = "__aarch64_cas2_relax": (I16, I16, PTR) -> I16;
    /// `__aarch64_cas2_acq`: outline atomics (WP-074).
    Cas2Acq = "__aarch64_cas2_acq": (I16, I16, PTR) -> I16;
    /// `__aarch64_cas2_rel`: outline atomics (WP-074).
    Cas2Rel = "__aarch64_cas2_rel": (I16, I16, PTR) -> I16;
    /// `__aarch64_cas2_acq_rel`: outline atomics (WP-074).
    Cas2AcqRel = "__aarch64_cas2_acq_rel": (I16, I16, PTR) -> I16;
    /// `__aarch64_cas4_relax`: outline atomics (WP-074).
    Cas4Relax = "__aarch64_cas4_relax": (I32, I32, PTR) -> I32;
    /// `__aarch64_cas4_acq`: outline atomics (WP-074).
    Cas4Acq = "__aarch64_cas4_acq": (I32, I32, PTR) -> I32;
    /// `__aarch64_cas4_rel`: outline atomics (WP-074).
    Cas4Rel = "__aarch64_cas4_rel": (I32, I32, PTR) -> I32;
    /// `__aarch64_cas4_acq_rel`: outline atomics (WP-074).
    Cas4AcqRel = "__aarch64_cas4_acq_rel": (I32, I32, PTR) -> I32;
    /// `__aarch64_cas8_relax`: outline atomics (WP-074).
    Cas8Relax = "__aarch64_cas8_relax": (I64, I64, PTR) -> I64;
    /// `__aarch64_cas8_acq`: outline atomics (WP-074).
    Cas8Acq = "__aarch64_cas8_acq": (I64, I64, PTR) -> I64;
    /// `__aarch64_cas8_rel`: outline atomics (WP-074).
    Cas8Rel = "__aarch64_cas8_rel": (I64, I64, PTR) -> I64;
    /// `__aarch64_cas8_acq_rel`: outline atomics (WP-074).
    Cas8AcqRel = "__aarch64_cas8_acq_rel": (I64, I64, PTR) -> I64;
    /// `__aarch64_cas16_relax`: outline atomics (WP-074).
    Cas16Relax = "__aarch64_cas16_relax": (I128, I128, PTR) -> I128;
    /// `__aarch64_cas16_acq`: outline atomics (WP-074).
    Cas16Acq = "__aarch64_cas16_acq": (I128, I128, PTR) -> I128;
    /// `__aarch64_cas16_rel`: outline atomics (WP-074).
    Cas16Rel = "__aarch64_cas16_rel": (I128, I128, PTR) -> I128;
    /// `__aarch64_cas16_acq_rel`: outline atomics (WP-074).
    Cas16AcqRel = "__aarch64_cas16_acq_rel": (I128, I128, PTR) -> I128;
    /// `__aarch64_swp1_relax`: outline atomics (WP-074).
    Swp1Relax = "__aarch64_swp1_relax": (I8, PTR) -> I8;
    /// `__aarch64_swp1_acq`: outline atomics (WP-074).
    Swp1Acq = "__aarch64_swp1_acq": (I8, PTR) -> I8;
    /// `__aarch64_swp1_rel`: outline atomics (WP-074).
    Swp1Rel = "__aarch64_swp1_rel": (I8, PTR) -> I8;
    /// `__aarch64_swp1_acq_rel`: outline atomics (WP-074).
    Swp1AcqRel = "__aarch64_swp1_acq_rel": (I8, PTR) -> I8;
    /// `__aarch64_swp2_relax`: outline atomics (WP-074).
    Swp2Relax = "__aarch64_swp2_relax": (I16, PTR) -> I16;
    /// `__aarch64_swp2_acq`: outline atomics (WP-074).
    Swp2Acq = "__aarch64_swp2_acq": (I16, PTR) -> I16;
    /// `__aarch64_swp2_rel`: outline atomics (WP-074).
    Swp2Rel = "__aarch64_swp2_rel": (I16, PTR) -> I16;
    /// `__aarch64_swp2_acq_rel`: outline atomics (WP-074).
    Swp2AcqRel = "__aarch64_swp2_acq_rel": (I16, PTR) -> I16;
    /// `__aarch64_swp4_relax`: outline atomics (WP-074).
    Swp4Relax = "__aarch64_swp4_relax": (I32, PTR) -> I32;
    /// `__aarch64_swp4_acq`: outline atomics (WP-074).
    Swp4Acq = "__aarch64_swp4_acq": (I32, PTR) -> I32;
    /// `__aarch64_swp4_rel`: outline atomics (WP-074).
    Swp4Rel = "__aarch64_swp4_rel": (I32, PTR) -> I32;
    /// `__aarch64_swp4_acq_rel`: outline atomics (WP-074).
    Swp4AcqRel = "__aarch64_swp4_acq_rel": (I32, PTR) -> I32;
    /// `__aarch64_swp8_relax`: outline atomics (WP-074).
    Swp8Relax = "__aarch64_swp8_relax": (I64, PTR) -> I64;
    /// `__aarch64_swp8_acq`: outline atomics (WP-074).
    Swp8Acq = "__aarch64_swp8_acq": (I64, PTR) -> I64;
    /// `__aarch64_swp8_rel`: outline atomics (WP-074).
    Swp8Rel = "__aarch64_swp8_rel": (I64, PTR) -> I64;
    /// `__aarch64_swp8_acq_rel`: outline atomics (WP-074).
    Swp8AcqRel = "__aarch64_swp8_acq_rel": (I64, PTR) -> I64;
    /// `__aarch64_ldadd1_relax`: outline atomics (WP-074).
    Ldadd1Relax = "__aarch64_ldadd1_relax": (I8, PTR) -> I8;
    /// `__aarch64_ldadd1_acq`: outline atomics (WP-074).
    Ldadd1Acq = "__aarch64_ldadd1_acq": (I8, PTR) -> I8;
    /// `__aarch64_ldadd1_rel`: outline atomics (WP-074).
    Ldadd1Rel = "__aarch64_ldadd1_rel": (I8, PTR) -> I8;
    /// `__aarch64_ldadd1_acq_rel`: outline atomics (WP-074).
    Ldadd1AcqRel = "__aarch64_ldadd1_acq_rel": (I8, PTR) -> I8;
    /// `__aarch64_ldadd2_relax`: outline atomics (WP-074).
    Ldadd2Relax = "__aarch64_ldadd2_relax": (I16, PTR) -> I16;
    /// `__aarch64_ldadd2_acq`: outline atomics (WP-074).
    Ldadd2Acq = "__aarch64_ldadd2_acq": (I16, PTR) -> I16;
    /// `__aarch64_ldadd2_rel`: outline atomics (WP-074).
    Ldadd2Rel = "__aarch64_ldadd2_rel": (I16, PTR) -> I16;
    /// `__aarch64_ldadd2_acq_rel`: outline atomics (WP-074).
    Ldadd2AcqRel = "__aarch64_ldadd2_acq_rel": (I16, PTR) -> I16;
    /// `__aarch64_ldadd4_relax`: outline atomics (WP-074).
    Ldadd4Relax = "__aarch64_ldadd4_relax": (I32, PTR) -> I32;
    /// `__aarch64_ldadd4_acq`: outline atomics (WP-074).
    Ldadd4Acq = "__aarch64_ldadd4_acq": (I32, PTR) -> I32;
    /// `__aarch64_ldadd4_rel`: outline atomics (WP-074).
    Ldadd4Rel = "__aarch64_ldadd4_rel": (I32, PTR) -> I32;
    /// `__aarch64_ldadd4_acq_rel`: outline atomics (WP-074).
    Ldadd4AcqRel = "__aarch64_ldadd4_acq_rel": (I32, PTR) -> I32;
    /// `__aarch64_ldadd8_relax`: outline atomics (WP-074).
    Ldadd8Relax = "__aarch64_ldadd8_relax": (I64, PTR) -> I64;
    /// `__aarch64_ldadd8_acq`: outline atomics (WP-074).
    Ldadd8Acq = "__aarch64_ldadd8_acq": (I64, PTR) -> I64;
    /// `__aarch64_ldadd8_rel`: outline atomics (WP-074).
    Ldadd8Rel = "__aarch64_ldadd8_rel": (I64, PTR) -> I64;
    /// `__aarch64_ldadd8_acq_rel`: outline atomics (WP-074).
    Ldadd8AcqRel = "__aarch64_ldadd8_acq_rel": (I64, PTR) -> I64;
    /// `__aarch64_ldclr1_relax`: outline atomics (WP-074).
    Ldclr1Relax = "__aarch64_ldclr1_relax": (I8, PTR) -> I8;
    /// `__aarch64_ldclr1_acq`: outline atomics (WP-074).
    Ldclr1Acq = "__aarch64_ldclr1_acq": (I8, PTR) -> I8;
    /// `__aarch64_ldclr1_rel`: outline atomics (WP-074).
    Ldclr1Rel = "__aarch64_ldclr1_rel": (I8, PTR) -> I8;
    /// `__aarch64_ldclr1_acq_rel`: outline atomics (WP-074).
    Ldclr1AcqRel = "__aarch64_ldclr1_acq_rel": (I8, PTR) -> I8;
    /// `__aarch64_ldclr2_relax`: outline atomics (WP-074).
    Ldclr2Relax = "__aarch64_ldclr2_relax": (I16, PTR) -> I16;
    /// `__aarch64_ldclr2_acq`: outline atomics (WP-074).
    Ldclr2Acq = "__aarch64_ldclr2_acq": (I16, PTR) -> I16;
    /// `__aarch64_ldclr2_rel`: outline atomics (WP-074).
    Ldclr2Rel = "__aarch64_ldclr2_rel": (I16, PTR) -> I16;
    /// `__aarch64_ldclr2_acq_rel`: outline atomics (WP-074).
    Ldclr2AcqRel = "__aarch64_ldclr2_acq_rel": (I16, PTR) -> I16;
    /// `__aarch64_ldclr4_relax`: outline atomics (WP-074).
    Ldclr4Relax = "__aarch64_ldclr4_relax": (I32, PTR) -> I32;
    /// `__aarch64_ldclr4_acq`: outline atomics (WP-074).
    Ldclr4Acq = "__aarch64_ldclr4_acq": (I32, PTR) -> I32;
    /// `__aarch64_ldclr4_rel`: outline atomics (WP-074).
    Ldclr4Rel = "__aarch64_ldclr4_rel": (I32, PTR) -> I32;
    /// `__aarch64_ldclr4_acq_rel`: outline atomics (WP-074).
    Ldclr4AcqRel = "__aarch64_ldclr4_acq_rel": (I32, PTR) -> I32;
    /// `__aarch64_ldclr8_relax`: outline atomics (WP-074).
    Ldclr8Relax = "__aarch64_ldclr8_relax": (I64, PTR) -> I64;
    /// `__aarch64_ldclr8_acq`: outline atomics (WP-074).
    Ldclr8Acq = "__aarch64_ldclr8_acq": (I64, PTR) -> I64;
    /// `__aarch64_ldclr8_rel`: outline atomics (WP-074).
    Ldclr8Rel = "__aarch64_ldclr8_rel": (I64, PTR) -> I64;
    /// `__aarch64_ldclr8_acq_rel`: outline atomics (WP-074).
    Ldclr8AcqRel = "__aarch64_ldclr8_acq_rel": (I64, PTR) -> I64;
    /// `__aarch64_ldeor1_relax`: outline atomics (WP-074).
    Ldeor1Relax = "__aarch64_ldeor1_relax": (I8, PTR) -> I8;
    /// `__aarch64_ldeor1_acq`: outline atomics (WP-074).
    Ldeor1Acq = "__aarch64_ldeor1_acq": (I8, PTR) -> I8;
    /// `__aarch64_ldeor1_rel`: outline atomics (WP-074).
    Ldeor1Rel = "__aarch64_ldeor1_rel": (I8, PTR) -> I8;
    /// `__aarch64_ldeor1_acq_rel`: outline atomics (WP-074).
    Ldeor1AcqRel = "__aarch64_ldeor1_acq_rel": (I8, PTR) -> I8;
    /// `__aarch64_ldeor2_relax`: outline atomics (WP-074).
    Ldeor2Relax = "__aarch64_ldeor2_relax": (I16, PTR) -> I16;
    /// `__aarch64_ldeor2_acq`: outline atomics (WP-074).
    Ldeor2Acq = "__aarch64_ldeor2_acq": (I16, PTR) -> I16;
    /// `__aarch64_ldeor2_rel`: outline atomics (WP-074).
    Ldeor2Rel = "__aarch64_ldeor2_rel": (I16, PTR) -> I16;
    /// `__aarch64_ldeor2_acq_rel`: outline atomics (WP-074).
    Ldeor2AcqRel = "__aarch64_ldeor2_acq_rel": (I16, PTR) -> I16;
    /// `__aarch64_ldeor4_relax`: outline atomics (WP-074).
    Ldeor4Relax = "__aarch64_ldeor4_relax": (I32, PTR) -> I32;
    /// `__aarch64_ldeor4_acq`: outline atomics (WP-074).
    Ldeor4Acq = "__aarch64_ldeor4_acq": (I32, PTR) -> I32;
    /// `__aarch64_ldeor4_rel`: outline atomics (WP-074).
    Ldeor4Rel = "__aarch64_ldeor4_rel": (I32, PTR) -> I32;
    /// `__aarch64_ldeor4_acq_rel`: outline atomics (WP-074).
    Ldeor4AcqRel = "__aarch64_ldeor4_acq_rel": (I32, PTR) -> I32;
    /// `__aarch64_ldeor8_relax`: outline atomics (WP-074).
    Ldeor8Relax = "__aarch64_ldeor8_relax": (I64, PTR) -> I64;
    /// `__aarch64_ldeor8_acq`: outline atomics (WP-074).
    Ldeor8Acq = "__aarch64_ldeor8_acq": (I64, PTR) -> I64;
    /// `__aarch64_ldeor8_rel`: outline atomics (WP-074).
    Ldeor8Rel = "__aarch64_ldeor8_rel": (I64, PTR) -> I64;
    /// `__aarch64_ldeor8_acq_rel`: outline atomics (WP-074).
    Ldeor8AcqRel = "__aarch64_ldeor8_acq_rel": (I64, PTR) -> I64;
    /// `__aarch64_ldset1_relax`: outline atomics (WP-074).
    Ldset1Relax = "__aarch64_ldset1_relax": (I8, PTR) -> I8;
    /// `__aarch64_ldset1_acq`: outline atomics (WP-074).
    Ldset1Acq = "__aarch64_ldset1_acq": (I8, PTR) -> I8;
    /// `__aarch64_ldset1_rel`: outline atomics (WP-074).
    Ldset1Rel = "__aarch64_ldset1_rel": (I8, PTR) -> I8;
    /// `__aarch64_ldset1_acq_rel`: outline atomics (WP-074).
    Ldset1AcqRel = "__aarch64_ldset1_acq_rel": (I8, PTR) -> I8;
    /// `__aarch64_ldset2_relax`: outline atomics (WP-074).
    Ldset2Relax = "__aarch64_ldset2_relax": (I16, PTR) -> I16;
    /// `__aarch64_ldset2_acq`: outline atomics (WP-074).
    Ldset2Acq = "__aarch64_ldset2_acq": (I16, PTR) -> I16;
    /// `__aarch64_ldset2_rel`: outline atomics (WP-074).
    Ldset2Rel = "__aarch64_ldset2_rel": (I16, PTR) -> I16;
    /// `__aarch64_ldset2_acq_rel`: outline atomics (WP-074).
    Ldset2AcqRel = "__aarch64_ldset2_acq_rel": (I16, PTR) -> I16;
    /// `__aarch64_ldset4_relax`: outline atomics (WP-074).
    Ldset4Relax = "__aarch64_ldset4_relax": (I32, PTR) -> I32;
    /// `__aarch64_ldset4_acq`: outline atomics (WP-074).
    Ldset4Acq = "__aarch64_ldset4_acq": (I32, PTR) -> I32;
    /// `__aarch64_ldset4_rel`: outline atomics (WP-074).
    Ldset4Rel = "__aarch64_ldset4_rel": (I32, PTR) -> I32;
    /// `__aarch64_ldset4_acq_rel`: outline atomics (WP-074).
    Ldset4AcqRel = "__aarch64_ldset4_acq_rel": (I32, PTR) -> I32;
    /// `__aarch64_ldset8_relax`: outline atomics (WP-074).
    Ldset8Relax = "__aarch64_ldset8_relax": (I64, PTR) -> I64;
    /// `__aarch64_ldset8_acq`: outline atomics (WP-074).
    Ldset8Acq = "__aarch64_ldset8_acq": (I64, PTR) -> I64;
    /// `__aarch64_ldset8_rel`: outline atomics (WP-074).
    Ldset8Rel = "__aarch64_ldset8_rel": (I64, PTR) -> I64;
    /// `__aarch64_ldset8_acq_rel`: outline atomics (WP-074).
    Ldset8AcqRel = "__aarch64_ldset8_acq_rel": (I64, PTR) -> I64;
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
