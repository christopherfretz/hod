//! What code refers to: module symbols and runtime functions
//! (`spec/codegen.md` §7.3, §9.2).

use hod_ir::{Arch, FloatPredicate, Opcode, SymId, TypeId, TypeKind};

/// The target of a relocation in a function's code (§9.2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SymRef {
    /// A symbol of the module.
    Module(SymId),
    /// A function that is not in the module (§7.3). Module emission maps
    /// it to the module's own symbol of that name if it declares one, and
    /// to a new undefined global symbol otherwise.
    Runtime(RuntimeFn),
    /// The object's noop coroutine frame (`llvm.coro.noop`, WP-105,
    /// `spec/codegen.md` "Coroutines"): a private constant that module
    /// emission places, with the function it points to, on first use.
    NoopCoroFrame,
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
            /// list needs no type table. A `long double` routine of libm
            /// (`sinl`, `fmodl`, ...) is described with AArch64's `long
            /// double`, `fp128`; on x86-64 the same symbol takes and returns
            /// `x86_fp80` ([`fp_routine`]).
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
    /// `fptosi half` to at most 128 bits (WP-084).
    FixHfTi = "__fixhfti": (HALF) -> I128;
    /// `fptosi float` to at most 128 bits (WP-084).
    FixSfTi = "__fixsfti": (FLOAT) -> I128;
    /// `fptosi double` to at most 128 bits (WP-084).
    FixDfTi = "__fixdfti": (DOUBLE) -> I128;
    /// `fptosi fp128` to at most 128 bits (WP-084).
    FixTfTi = "__fixtfti": (FP128) -> I128;
    /// `fptoui half` to at most 128 bits (WP-084).
    FixunsHfTi = "__fixunshfti": (HALF) -> I128;
    /// `fptoui float` to at most 128 bits (WP-084).
    FixunsSfTi = "__fixunssfti": (FLOAT) -> I128;
    /// `fptoui double` to at most 128 bits (WP-084).
    FixunsDfTi = "__fixunsdfti": (DOUBLE) -> I128;
    /// `fptoui fp128` to at most 128 bits (WP-084).
    FixunsTfTi = "__fixunstfti": (FP128) -> I128;
    /// `sitofp` of at most 128 bits to `float` (and `half` through it)
    /// (WP-084).
    FloatTiSf = "__floattisf": (I128) -> FLOAT;
    /// `sitofp` of at most 128 bits to `double` (WP-084).
    FloatTiDf = "__floattidf": (I128) -> DOUBLE;
    /// `sitofp` of at most 128 bits to `fp128` (WP-084).
    FloatTiTf = "__floattitf": (I128) -> FP128;
    /// `uitofp` of at most 128 bits to `float` (and `half` through it)
    /// (WP-084).
    FloatunTiSf = "__floatuntisf": (I128) -> FLOAT;
    /// `uitofp` of at most 128 bits to `double` (WP-084).
    FloatunTiDf = "__floatuntidf": (I128) -> DOUBLE;
    /// `uitofp` of at most 128 bits to `fp128` (WP-084).
    FloatunTiTf = "__floatuntitf": (I128) -> FP128;
    /// `fpext half` to `float` (x86-64; WP-117).
    ExtendHfSf2 = "__extendhfsf2": (HALF) -> FLOAT;
    /// `fptrunc float` to `half` (x86-64; WP-117).
    TruncSfHf2 = "__truncsfhf2": (FLOAT) -> HALF;
    /// `fptrunc double` to `half` (x86-64; WP-117).
    TruncDfHf2 = "__truncdfhf2": (DOUBLE) -> HALF;
    /// `fpext half` to `fp128` (x86-64; WP-117).
    ExtendHfTf2 = "__extendhftf2": (HALF) -> FP128;
    /// `fpext half` to `x86_fp80` (x86-64; WP-117).
    ExtendHfXf2 = "__extendhfxf2": (HALF) -> X86_FP80;
    /// `fptrunc x86_fp80` to `half` (x86-64; WP-117).
    TruncXfHf2 = "__truncxfhf2": (X86_FP80) -> HALF;
    /// `fptrunc x86_fp80` to `bfloat` (x86-64; WP-147): libgcc since
    /// GCC 13 and compiler-rt have it.
    TruncXfBf2 = "__truncxfbf2": (X86_FP80) -> BFLOAT;
    /// `fpext x86_fp80` to `fp128` (x86-64; WP-117).
    ExtendXfTf2 = "__extendxftf2": (X86_FP80) -> FP128;
    /// `fptrunc fp128` to `x86_fp80` (x86-64; WP-117).
    TruncTfXf2 = "__trunctfxf2": (FP128) -> X86_FP80;
    /// `fptosi x86_fp80` to at most 128 bits (x86-64; WP-117).
    FixXfTi = "__fixxfti": (X86_FP80) -> I128;
    /// `fptoui x86_fp80` to at most 128 bits (x86-64; WP-117).
    FixunsXfTi = "__fixunsxfti": (X86_FP80) -> I128;
    /// `sitofp` of at most 128 bits to `x86_fp80` (x86-64; WP-117).
    FloatTiXf = "__floattixf": (I128) -> X86_FP80;
    /// `uitofp` of at most 128 bits to `x86_fp80` (x86-64; WP-117).
    FloatunTiXf = "__floatuntixf": (I128) -> X86_FP80;
    /// `sitofp` of at most 128 bits to `half` (x86-64; WP-117).
    FloatTiHf = "__floattihf": (I128) -> HALF;
    /// `uitofp` of at most 128 bits to `half` (x86-64; WP-117).
    FloatunTiHf = "__floatuntihf": (I128) -> HALF;
    /// `llvm.powi.f80` (x86-64; WP-117).
    PowiXf2 = "__powixf2": (X86_FP80, I32) -> X86_FP80;
    /// `llvm.floor.f32`, a call on x86-64 (WP-117).
    Floorf = "floorf": (FLOAT) -> FLOAT;
    /// `llvm.floor.f64`, a call on x86-64 (WP-117).
    Floor = "floor": (DOUBLE) -> DOUBLE;
    /// `llvm.ceil.f32`, a call on x86-64 (WP-117).
    Ceilf = "ceilf": (FLOAT) -> FLOAT;
    /// `llvm.ceil.f64`, a call on x86-64 (WP-117).
    Ceil = "ceil": (DOUBLE) -> DOUBLE;
    /// `llvm.trunc.f32`, a call on x86-64 (WP-117).
    Truncf = "truncf": (FLOAT) -> FLOAT;
    /// `llvm.trunc.f64`, a call on x86-64 (WP-117).
    Trunc = "trunc": (DOUBLE) -> DOUBLE;
    /// `llvm.round.f32`, a call on x86-64 (WP-117).
    Roundf = "roundf": (FLOAT) -> FLOAT;
    /// `llvm.round.f64`, a call on x86-64 (WP-117).
    Round = "round": (DOUBLE) -> DOUBLE;
    /// `llvm.roundeven.f32`, a call on x86-64 (WP-117).
    Roundevenf = "roundevenf": (FLOAT) -> FLOAT;
    /// `llvm.roundeven.f64`, a call on x86-64 (WP-117).
    Roundeven = "roundeven": (DOUBLE) -> DOUBLE;
    /// `llvm.nearbyint.f32`, a call on x86-64 (WP-117).
    Nearbyintf = "nearbyintf": (FLOAT) -> FLOAT;
    /// `llvm.nearbyint.f64`, a call on x86-64 (WP-117).
    Nearbyint = "nearbyint": (DOUBLE) -> DOUBLE;
    /// `llvm.rint.f32`, a call on x86-64 (WP-117).
    Rintf = "rintf": (FLOAT) -> FLOAT;
    /// `llvm.rint.f64`, a call on x86-64 (WP-117).
    Rint = "rint": (DOUBLE) -> DOUBLE;
    /// `llvm.fma.f32`, a call on x86-64 (WP-117).
    Fmaf = "fmaf": (FLOAT, FLOAT, FLOAT) -> FLOAT;
    /// `llvm.fma.f64`, a call on x86-64 (WP-117).
    Fma = "fma": (DOUBLE, DOUBLE, DOUBLE) -> DOUBLE;
    /// `llvm.lround.f32`, a call on x86-64 (WP-117).
    Lroundf = "lroundf": (FLOAT) -> I64;
    /// `llvm.lround.f64`, a call on x86-64 (WP-117).
    Lround = "lround": (DOUBLE) -> I64;
    /// `llvm.llround.f32`, a call on x86-64 (WP-117).
    Llroundf = "llroundf": (FLOAT) -> I64;
    /// `llvm.llround.f64`, a call on x86-64 (WP-117).
    Llround = "llround": (DOUBLE) -> I64;
    /// `llvm.minnum.f16` on x86-64, through `float` (WP-117).
    Fminf = "fminf": (FLOAT, FLOAT) -> FLOAT;
    /// `llvm.maxnum.f16` on x86-64, through `float` (WP-117).
    Fmaxf = "fmaxf": (FLOAT, FLOAT) -> FLOAT;
    /// `sinf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Sinf128 = "sinf128": (FP128) -> FP128;
    /// `cosf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Cosf128 = "cosf128": (FP128) -> FP128;
    /// `tanf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Tanf128 = "tanf128": (FP128) -> FP128;
    /// `asinf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Asinf128 = "asinf128": (FP128) -> FP128;
    /// `acosf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Acosf128 = "acosf128": (FP128) -> FP128;
    /// `atanf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Atanf128 = "atanf128": (FP128) -> FP128;
    /// `atan2f128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Atan2f128 = "atan2f128": (FP128, FP128) -> FP128;
    /// `sinhf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Sinhf128 = "sinhf128": (FP128) -> FP128;
    /// `coshf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Coshf128 = "coshf128": (FP128) -> FP128;
    /// `tanhf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Tanhf128 = "tanhf128": (FP128) -> FP128;
    /// `expf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Expf128 = "expf128": (FP128) -> FP128;
    /// `exp2f128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Exp2f128 = "exp2f128": (FP128) -> FP128;
    /// `exp10f128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Exp10f128 = "exp10f128": (FP128) -> FP128;
    /// `logf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Logf128 = "logf128": (FP128) -> FP128;
    /// `log2f128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Log2f128 = "log2f128": (FP128) -> FP128;
    /// `log10f128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Log10f128 = "log10f128": (FP128) -> FP128;
    /// `powf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Powf128 = "powf128": (FP128, FP128) -> FP128;
    /// `fmodf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Fmodf128 = "fmodf128": (FP128, FP128) -> FP128;
    /// `sqrtf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Sqrtf128 = "sqrtf128": (FP128) -> FP128;
    /// `floorf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Floorf128 = "floorf128": (FP128) -> FP128;
    /// `ceilf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Ceilf128 = "ceilf128": (FP128) -> FP128;
    /// `truncf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Truncf128 = "truncf128": (FP128) -> FP128;
    /// `roundf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Roundf128 = "roundf128": (FP128) -> FP128;
    /// `roundevenf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Roundevenf128 = "roundevenf128": (FP128) -> FP128;
    /// `nearbyintf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Nearbyintf128 = "nearbyintf128": (FP128) -> FP128;
    /// `rintf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Rintf128 = "rintf128": (FP128) -> FP128;
    /// `fmaf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Fmaf128 = "fmaf128": (FP128, FP128, FP128) -> FP128;
    /// `fminf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Fminf128 = "fminf128": (FP128, FP128) -> FP128;
    /// `fmaxf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Fmaxf128 = "fmaxf128": (FP128, FP128) -> FP128;
    /// `fminimumf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Fminimumf128 = "fminimumf128": (FP128, FP128) -> FP128;
    /// `fmaximumf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Fmaximumf128 = "fmaximumf128": (FP128, FP128) -> FP128;
    /// `fminimum_numf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    FminimumNumf128 = "fminimum_numf128": (FP128, FP128) -> FP128;
    /// `fmaximum_numf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    FmaximumNumf128 = "fmaximum_numf128": (FP128, FP128) -> FP128;
    /// `lroundf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Lroundf128 = "lroundf128": (FP128) -> I64;
    /// `llroundf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Llroundf128 = "llroundf128": (FP128) -> I64;
    /// `lrintf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Lrintf128 = "lrintf128": (FP128) -> I64;
    /// `llrintf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Llrintf128 = "llrintf128": (FP128) -> I64;
    /// `ldexpf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Ldexpf128 = "ldexpf128": (FP128, I32) -> FP128;
    /// `frexpf128`: `fp128` on x86-64, where `long double` is `x86_fp80` (WP-117).
    Frexpf128 = "frexpf128": (FP128, PTR) -> FP128;
    /// `__atomic_load_16(ptr, order)`: libatomic, a 16-byte atomic on
    /// x86-64 without `cx16` (WP-144).
    AtomicLoad16 = "__atomic_load_16": (PTR, I32) -> I128;
    /// `__atomic_store_16(ptr, value, order)` (WP-144).
    AtomicStore16 = "__atomic_store_16": (PTR, I128, I32) -> VOID;
    /// `__atomic_exchange_16(ptr, value, order)` (WP-144).
    AtomicExchange16 = "__atomic_exchange_16": (PTR, I128, I32) -> I128;
    /// `__atomic_compare_exchange_16(ptr, expected *, desired, success,
    /// failure)`: C `bool` (WP-144).
    AtomicCompareExchange16 = "__atomic_compare_exchange_16": (PTR, PTR, I128, I32, I32) -> I8;
    /// `__atomic_fetch_add_16(ptr, value, order)` (WP-144).
    AtomicFetchAdd16 = "__atomic_fetch_add_16": (PTR, I128, I32) -> I128;
    /// `__atomic_fetch_sub_16(ptr, value, order)` (WP-144).
    AtomicFetchSub16 = "__atomic_fetch_sub_16": (PTR, I128, I32) -> I128;
    /// `__atomic_fetch_and_16(ptr, value, order)` (WP-144).
    AtomicFetchAnd16 = "__atomic_fetch_and_16": (PTR, I128, I32) -> I128;
    /// `__atomic_fetch_nand_16(ptr, value, order)` (WP-144).
    AtomicFetchNand16 = "__atomic_fetch_nand_16": (PTR, I128, I32) -> I128;
    /// `__atomic_fetch_or_16(ptr, value, order)` (WP-144).
    AtomicFetchOr16 = "__atomic_fetch_or_16": (PTR, I128, I32) -> I128;
    /// `__atomic_fetch_xor_16(ptr, value, order)` (WP-144).
    AtomicFetchXor16 = "__atomic_fetch_xor_16": (PTR, I128, I32) -> I128;
}

// ---- Runtime routines keyed by (type, target): §7.3 (WP-117) ----

/// A floating-point type, as runtime routines are keyed by it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FpType {
    /// `half`.
    Half,
    /// `float`.
    Float,
    /// `double`.
    Double,
    /// `fp128`: `long double` on AArch64, `__float128` on x86-64.
    Fp128,
    /// `x86_fp80`: `long double` on x86-64; no routine on AArch64.
    X86Fp80,
}

impl FpType {
    /// The type of `kind`, if it is one of these.
    pub fn of(kind: TypeKind<'_>) -> Option<FpType> {
        Some(match kind {
            TypeKind::Half => FpType::Half,
            TypeKind::Float => FpType::Float,
            TypeKind::Double => FpType::Double,
            TypeKind::Fp128 => FpType::Fp128,
            TypeKind::X86Fp80 => FpType::X86Fp80,
            _ => return None,
        })
    }
}

/// A floating-point operation that may be a call of a runtime routine:
/// the arithmetic instructions and the intrinsics of the same names.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[allow(missing_docs, reason = "each is the instruction or intrinsic of its name")]
pub enum FpOp {
    Add,
    Sub,
    Mul,
    Div,
    /// `frem`.
    Rem,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Sinh,
    Cosh,
    Tanh,
    Exp,
    Exp2,
    Exp10,
    Log,
    Log2,
    Log10,
    Pow,
    Sqrt,
    Floor,
    Ceil,
    Trunc,
    Round,
    RoundEven,
    NearbyInt,
    Rint,
    Fma,
    MinNum,
    MaxNum,
    Minimum,
    Maximum,
    MinimumNum,
    MaximumNum,
    Lround,
    Llround,
    Lrint,
    Llrint,
    Powi,
    Ldexp,
    Frexp,
}

impl FpOp {
    /// Every operation.
    pub const ALL: [FpOp; 44] = {
        use FpOp as O;
        [
            O::Add,
            O::Sub,
            O::Mul,
            O::Div,
            O::Rem,
            O::Sin,
            O::Cos,
            O::Tan,
            O::Asin,
            O::Acos,
            O::Atan,
            O::Atan2,
            O::Sinh,
            O::Cosh,
            O::Tanh,
            O::Exp,
            O::Exp2,
            O::Exp10,
            O::Log,
            O::Log2,
            O::Log10,
            O::Pow,
            O::Sqrt,
            O::Floor,
            O::Ceil,
            O::Trunc,
            O::Round,
            O::RoundEven,
            O::NearbyInt,
            O::Rint,
            O::Fma,
            O::MinNum,
            O::MaxNum,
            O::Minimum,
            O::Maximum,
            O::MinimumNum,
            O::MaximumNum,
            O::Lround,
            O::Llround,
            O::Lrint,
            O::Llrint,
            O::Powi,
            O::Ldexp,
            O::Frexp,
        ]
    };
}

/// The `float` and `double` routines of `op`, where libm has them.
fn short_row(op: FpOp) -> Option<[RuntimeFn; 2]> {
    use FpOp as O;
    use RuntimeFn as R;
    Some(match op {
        O::Rem => [R::Fmodf, R::Fmod],
        O::Sin => [R::Sinf, R::Sin],
        O::Cos => [R::Cosf, R::Cos],
        O::Tan => [R::Tanf, R::Tan],
        O::Asin => [R::Asinf, R::Asin],
        O::Acos => [R::Acosf, R::Acos],
        O::Atan => [R::Atanf, R::Atan],
        O::Atan2 => [R::Atan2f, R::Atan2],
        O::Sinh => [R::Sinhf, R::Sinh],
        O::Cosh => [R::Coshf, R::Cosh],
        O::Tanh => [R::Tanhf, R::Tanh],
        O::Exp => [R::Expf, R::Exp],
        O::Exp2 => [R::Exp2f, R::Exp2],
        O::Exp10 => [R::Exp10f, R::Exp10],
        O::Log => [R::Logf, R::Log],
        O::Log2 => [R::Log2f, R::Log2],
        O::Log10 => [R::Log10f, R::Log10],
        O::Pow => [R::Powf, R::Pow],
        O::Powi => [R::PowiSf2, R::PowiDf2],
        O::Ldexp => [R::Ldexpf, R::Ldexp],
        O::Frexp => [R::Frexpf, R::Frexp],
        O::Floor => [R::Floorf, R::Floor],
        O::Ceil => [R::Ceilf, R::Ceil],
        O::Trunc => [R::Truncf, R::Trunc],
        O::Round => [R::Roundf, R::Round],
        O::RoundEven => [R::Roundevenf, R::Roundeven],
        O::NearbyInt => [R::Nearbyintf, R::Nearbyint],
        O::Rint => [R::Rintf, R::Rint],
        O::Fma => [R::Fmaf, R::Fma],
        O::Lround => [R::Lroundf, R::Lround],
        O::Llround => [R::Llroundf, R::Llround],
        _ => return None,
    })
}

/// The `long double` (`...l`) and `__float128` (`...f128`) routines of
/// `op`, except the arithmetic.
fn long_row(op: FpOp) -> Option<[RuntimeFn; 2]> {
    use FpOp as O;
    use RuntimeFn as R;
    Some(match op {
        O::Rem => [R::Fmodl, R::Fmodf128],
        O::Sin => [R::Sinl, R::Sinf128],
        O::Cos => [R::Cosl, R::Cosf128],
        O::Tan => [R::Tanl, R::Tanf128],
        O::Asin => [R::Asinl, R::Asinf128],
        O::Acos => [R::Acosl, R::Acosf128],
        O::Atan => [R::Atanl, R::Atanf128],
        O::Atan2 => [R::Atan2l, R::Atan2f128],
        O::Sinh => [R::Sinhl, R::Sinhf128],
        O::Cosh => [R::Coshl, R::Coshf128],
        O::Tanh => [R::Tanhl, R::Tanhf128],
        O::Exp => [R::Expl, R::Expf128],
        O::Exp2 => [R::Exp2l, R::Exp2f128],
        O::Exp10 => [R::Exp10l, R::Exp10f128],
        O::Log => [R::Logl, R::Logf128],
        O::Log2 => [R::Log2l, R::Log2f128],
        O::Log10 => [R::Log10l, R::Log10f128],
        O::Pow => [R::Powl, R::Powf128],
        O::Powi => [R::PowiTf2, R::PowiTf2],
        O::Ldexp => [R::Ldexpl, R::Ldexpf128],
        O::Frexp => [R::Frexpl, R::Frexpf128],
        O::Sqrt => [R::Sqrtl, R::Sqrtf128],
        O::Floor => [R::Floorl, R::Floorf128],
        O::Ceil => [R::Ceill, R::Ceilf128],
        O::Trunc => [R::Truncl, R::Truncf128],
        O::Round => [R::Roundl, R::Roundf128],
        O::RoundEven => [R::Roundevenl, R::Roundevenf128],
        O::NearbyInt => [R::Nearbyintl, R::Nearbyintf128],
        O::Rint => [R::Rintl, R::Rintf128],
        O::Fma => [R::Fmal, R::Fmaf128],
        O::MinNum => [R::Fminl, R::Fminf128],
        O::MaxNum => [R::Fmaxl, R::Fmaxf128],
        O::Minimum => [R::Fminimuml, R::Fminimumf128],
        O::Maximum => [R::Fmaximuml, R::Fmaximumf128],
        O::MinimumNum => [R::FminimumNuml, R::FminimumNumf128],
        O::MaximumNum => [R::FmaximumNuml, R::FmaximumNumf128],
        O::Lround => [R::Lroundl, R::Lroundf128],
        O::Llround => [R::Llroundl, R::Llroundf128],
        O::Lrint => [R::Lrintl, R::Lrintf128],
        O::Llrint => [R::Llrintl, R::Llrintf128],
        O::Add | O::Sub | O::Mul | O::Div => return None,
    })
}

/// The libm operations: those every type calls on every target.
fn is_libm(op: FpOp) -> bool {
    use FpOp as O;
    matches!(
        op,
        O::Sin
            | O::Cos
            | O::Tan
            | O::Asin
            | O::Acos
            | O::Atan
            | O::Atan2
            | O::Sinh
            | O::Cosh
            | O::Tanh
            | O::Exp
            | O::Exp2
            | O::Exp10
            | O::Log
            | O::Log2
            | O::Log10
            | O::Pow
    )
}

/// The routine `op` of `ty` calls on `arch`, as LLVM 22's `llc` calls it
/// (`spec/codegen.md` §7.3, "Runtime routines by type and target");
/// `None` when the target computes it inline, or refuses it (the
/// target's decision, by its own name).
///
/// - `fp128` is AArch64's `long double`: `sinl`, `fmodl`, `__addtf3`, ...
///   On x86-64 it is `__float128`: `sinf128`, `fmodf128`, and the same
///   `__addtf3` family. `x86_fp80` is x86-64's `long double`: `sinl`,
///   `fmodl`, `__powixf2`; its arithmetic, `sqrt`, `lrint` and `llrint`
///   are x87 instructions.
/// - `half` calls the `float` routine (widened, the result narrowed),
///   except: on x86-64 `fma` calls `fma` (LLVM 22 widens to `double`),
///   `minnum`/`maxnum` call `fminf`/`fmaxf` although `float` does not,
///   and `frexp`, `lround` .. `llrint` are no single call.
/// - On AArch64 `float` and `double` call only the libm functions,
///   `frem`, `powi`, `ldexp` and `frexp`; everything else is an
///   instruction. On x86-64 (SSE2, no SSE4.1, no FMA) the rounding
///   functions, `fma`, `lround` and `llround` are calls too.
/// - hod's choice where `llc` has none: `minimum`, `maximum`,
///   `minimumnum` and `maximumnum` of `fp128` and `x86_fp80` call the C23
///   functions (`fminimuml`, `fminimum_numl`, `fminimumf128`, ...;
///   `llc` cannot select the first two and expands the others inline).
pub fn fp_routine(op: FpOp, ty: FpType, arch: Arch) -> Option<RuntimeFn> {
    use FpOp as O;
    let column = usize::from(ty == FpType::Double);
    match (arch, ty) {
        (_, FpType::Fp128) => Some(match op {
            O::Add => RuntimeFn::AddTf3,
            O::Sub => RuntimeFn::SubTf3,
            O::Mul => RuntimeFn::MulTf3,
            O::Div => RuntimeFn::DivTf3,
            _ => return long_row(op).map(|row| row[usize::from(arch == Arch::X86_64)]),
        }),
        (Arch::Aarch64, FpType::X86Fp80) => None,
        (Arch::X86_64, FpType::X86Fp80) => match op {
            O::Sqrt | O::Lrint | O::Llrint => None,
            O::Powi => Some(RuntimeFn::PowiXf2),
            _ => long_row(op).map(|row| row[0]),
        },
        (Arch::Aarch64, _) => match op {
            O::Rem | O::Powi | O::Ldexp | O::Frexp => short_row(op).map(|row| row[column]),
            _ if is_libm(op) => short_row(op).map(|row| row[column]),
            _ => None,
        },
        (Arch::X86_64, FpType::Half) => match op {
            O::Fma => Some(RuntimeFn::Fma),
            O::MinNum => Some(RuntimeFn::Fminf),
            O::MaxNum => Some(RuntimeFn::Fmaxf),
            O::Frexp | O::Lround | O::Llround => None,
            _ => short_row(op).map(|row| row[0]),
        },
        (Arch::X86_64, _) => short_row(op).map(|row| row[column]),
    }
}

/// The routine of `fpext` from `from` to `to` on `arch`; `None` when it is
/// an instruction (or not valid IR).
///
/// AArch64 widens `half` to `float` before `__extendsftf2` (exact; `llc`
/// calls `__extendhftf2`). On x86-64 `half` to `double` is
/// `__extendhfsf2` then an instruction.
pub fn fp_ext_routine(from: FpType, to: FpType, arch: Arch) -> Option<RuntimeFn> {
    use FpType as F;
    use RuntimeFn as R;
    Some(match (arch, from, to) {
        (_, F::Float, F::Fp128) => R::ExtendSfTf2,
        (_, F::Double, F::Fp128) => R::ExtendDfTf2,
        (Arch::X86_64, F::Half, F::Float | F::Double) => R::ExtendHfSf2,
        (Arch::X86_64, F::Half, F::Fp128) => R::ExtendHfTf2,
        (Arch::X86_64, F::Half, F::X86Fp80) => R::ExtendHfXf2,
        (Arch::X86_64, F::X86Fp80, F::Fp128) => R::ExtendXfTf2,
        _ => return None,
    })
}

/// The routine of `fptrunc` from `from` to `to` on `arch`; `None` when it
/// is an instruction (or not valid IR).
pub fn fp_trunc_routine(from: FpType, to: FpType, arch: Arch) -> Option<RuntimeFn> {
    use FpType as F;
    use RuntimeFn as R;
    Some(match (arch, from, to) {
        (_, F::Fp128, F::Half) => R::TruncTfHf2,
        (_, F::Fp128, F::Float) => R::TruncTfSf2,
        (_, F::Fp128, F::Double) => R::TruncTfDf2,
        (Arch::X86_64, F::Float, F::Half) => R::TruncSfHf2,
        (Arch::X86_64, F::Double, F::Half) => R::TruncDfHf2,
        (Arch::X86_64, F::Fp128, F::X86Fp80) => R::TruncTfXf2,
        (Arch::X86_64, F::X86Fp80, F::Half) => R::TruncXfHf2,
        _ => return None,
    })
}

/// The routine of `fptosi` (`signed`) or `fptoui` from `from` to an
/// integer of `bits` bits on `arch`: `fp128` calls `__fix[uns]tf{si,di,ti}`
/// by width (up to 32, 64, 128 bits); the others call a routine only
/// above 64 bits (`__fix[uns]{hf,sf,df,xf}ti`). `None` for an instruction
/// and above 128 bits. On x86-64 a `half` up to 64 bits is widened by
/// `__extendhfsf2` first ([`fp_ext_routine`]).
pub fn fp_to_int_routine(from: FpType, signed: bool, bits: u32, arch: Arch) -> Option<RuntimeFn> {
    use FpType as F;
    use RuntimeFn as R;
    let width = match bits {
        1..=32 => 0,
        33..=64 => 1,
        65..=128 => 2,
        _ => return None,
    };
    let row = match (arch, from) {
        (_, F::Fp128) => {
            [[R::FixTfSi, R::FixTfDi, R::FixTfTi], [R::FixunsTfSi, R::FixunsTfDi, R::FixunsTfTi]]
        }
        (_, F::Half) if width == 2 => return Some(if signed { R::FixHfTi } else { R::FixunsHfTi }),
        (_, F::Float) if width == 2 => {
            return Some(if signed { R::FixSfTi } else { R::FixunsSfTi });
        }
        (_, F::Double) if width == 2 => {
            return Some(if signed { R::FixDfTi } else { R::FixunsDfTi });
        }
        (Arch::X86_64, F::X86Fp80) if width == 2 => {
            return Some(if signed { R::FixXfTi } else { R::FixunsXfTi });
        }
        _ => return None,
    };
    Some(row[usize::from(!signed)][width])
}

/// The routine of `sitofp` (`signed`) or `uitofp` from an integer of
/// `bits` bits to `to` on `arch`, as [`fp_to_int_routine`]. On AArch64 a
/// `half` from above 64 bits goes through `float` (`__floattisf`, then
/// an instruction); on x86-64 it calls `__floattihf`.
pub fn int_to_fp_routine(signed: bool, bits: u32, to: FpType, arch: Arch) -> Option<RuntimeFn> {
    use FpType as F;
    use RuntimeFn as R;
    let width = match bits {
        1..=32 => 0,
        33..=64 => 1,
        65..=128 => 2,
        _ => return None,
    };
    let pick = |s: R, u: R| Some(if signed { s } else { u });
    match (arch, to, width) {
        (_, F::Fp128, 0) => pick(R::FloatSiTf, R::FloatunSiTf),
        (_, F::Fp128, 1) => pick(R::FloatDiTf, R::FloatunDiTf),
        (_, F::Fp128, _) => pick(R::FloatTiTf, R::FloatunTiTf),
        (Arch::Aarch64, F::Half, 2) | (_, F::Float, 2) => pick(R::FloatTiSf, R::FloatunTiSf),
        (Arch::X86_64, F::Half, 2) => pick(R::FloatTiHf, R::FloatunTiHf),
        (_, F::Double, 2) => pick(R::FloatTiDf, R::FloatunTiDf),
        (Arch::X86_64, F::X86Fp80, 2) => pick(R::FloatTiXf, R::FloatunTiXf),
        _ => None,
    }
}

/// How the `int` result of an `fp128` comparison routine is compared
/// with 0 (signed).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[allow(missing_docs, reason = "the comparison of the result with zero")]
pub enum ZeroCmp {
    Eq,
    Ne,
    Lt,
    Ge,
    Le,
    Gt,
}

/// One call deciding `fcmp <pred> fp128` (both targets alike): the
/// routine, whether the operands are swapped, and the comparison of its
/// result with 0.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Fp128Compare {
    /// `__eqtf2`, `__lttf2`, `__letf2` or `__unordtf2`.
    pub func: RuntimeFn,
    /// The operands are passed in the other order.
    pub swap: bool,
    /// What the result is compared with 0 by.
    pub result: ZeroCmp,
}

/// `fcmp <pred> fp128` as LLVM's `softenSetCCOperands` computes it with
/// one call, the operands swapped for `ogt`, `oge`, `ule` and `ult` so
/// that only the signs of the `__eqtf2`, `__lttf2`, `__letf2` and
/// `__unordtf2` results are relied on (`__lttf2` and `__letf2` are
/// positive for unordered operands in libgcc and compiler-rt alike,
/// `__eqtf2` is 0 only for ordered equal ones). `None` for `ueq` and `one`
/// (two calls: `__eqtf2` and `__unordtf2`) and for `true` and `false`.
pub fn fp128_compare(pred: FloatPredicate) -> Option<Fp128Compare> {
    use FloatPredicate as P;
    use RuntimeFn::{EqTf2, LeTf2, LtTf2, UnordTf2};
    let (func, swap, result) = match pred {
        P::Oeq => (EqTf2, false, ZeroCmp::Eq),
        P::Une => (EqTf2, false, ZeroCmp::Ne),
        P::Olt => (LtTf2, false, ZeroCmp::Lt),
        P::Uge => (LtTf2, false, ZeroCmp::Ge),
        P::Ogt => (LtTf2, true, ZeroCmp::Lt),
        P::Ule => (LtTf2, true, ZeroCmp::Ge),
        P::Ole => (LeTf2, false, ZeroCmp::Le),
        P::Ugt => (LeTf2, false, ZeroCmp::Gt),
        P::Oge => (LeTf2, true, ZeroCmp::Le),
        P::Ult => (LeTf2, true, ZeroCmp::Gt),
        P::Ord => (UnordTf2, false, ZeroCmp::Eq),
        P::Uno => (UnordTf2, false, ZeroCmp::Ne),
        _ => return None,
    };
    Some(Fp128Compare { func, swap, result })
}

/// The routine of `udiv`, `sdiv`, `urem` or `srem` on an integer of 65 to
/// 128 bits (extended to 128 first), on both targets: `__udivti3`,
/// `__divti3`, `__umodti3`, `__modti3`. `None` for another opcode.
pub fn int_divide_routine(op: Opcode) -> Option<RuntimeFn> {
    Some(match op {
        Opcode::UDiv => RuntimeFn::UDivTi3,
        Opcode::SDiv => RuntimeFn::DivTi3,
        Opcode::URem => RuntimeFn::UModTi3,
        Opcode::SRem => RuntimeFn::ModTi3,
        _ => return None,
    })
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

    /// §7.3, "Runtime routines by type and target": `fp128` is `long
    /// double` on AArch64 and `__float128` on x86-64, where `x86_fp80` is
    /// `long double`.
    #[test]
    fn libm_names_by_type_and_target() {
        let name = |op, ty, arch| fp_routine(op, ty, arch).map(RuntimeFn::name);
        assert_eq!(name(FpOp::Sin, FpType::Fp128, Arch::X86_64), Some("sinf128"));
        assert_eq!(name(FpOp::Sin, FpType::X86Fp80, Arch::X86_64), Some("sinl"));
        assert_eq!(name(FpOp::Sin, FpType::Fp128, Arch::Aarch64), Some("sinl"));
        assert_eq!(name(FpOp::Sin, FpType::X86Fp80, Arch::Aarch64), None);
        assert_eq!(name(FpOp::Rem, FpType::Fp128, Arch::X86_64), Some("fmodf128"));
        assert_eq!(name(FpOp::Rem, FpType::X86Fp80, Arch::X86_64), Some("fmodl"));
        assert_eq!(name(FpOp::Rem, FpType::Fp128, Arch::Aarch64), Some("fmodl"));
        assert_eq!(name(FpOp::Rem, FpType::Half, Arch::Aarch64), Some("fmodf"));
        assert_eq!(name(FpOp::Add, FpType::Fp128, Arch::X86_64), Some("__addtf3"));
        assert_eq!(name(FpOp::Add, FpType::X86Fp80, Arch::X86_64), None);
        assert_eq!(name(FpOp::Sqrt, FpType::X86Fp80, Arch::X86_64), None);
        assert_eq!(name(FpOp::Powi, FpType::X86Fp80, Arch::X86_64), Some("__powixf2"));
        // The rounding functions: instructions on AArch64, calls on x86-64
        // without SSE4.1.
        assert_eq!(name(FpOp::Floor, FpType::Double, Arch::X86_64), Some("floor"));
        assert_eq!(name(FpOp::Floor, FpType::Float, Arch::X86_64), Some("floorf"));
        assert_eq!(name(FpOp::Floor, FpType::Half, Arch::X86_64), Some("floorf"));
        assert_eq!(name(FpOp::Floor, FpType::Double, Arch::Aarch64), None);
        assert_eq!(name(FpOp::Floor, FpType::Fp128, Arch::Aarch64), Some("floorl"));
        assert_eq!(name(FpOp::Sin, FpType::Half, Arch::Aarch64), Some("sinf"));
        assert_eq!(name(FpOp::MinimumNum, FpType::Fp128, Arch::Aarch64), Some("fminimum_numl"));
        assert_eq!(name(FpOp::MinimumNum, FpType::Fp128, Arch::X86_64), Some("fminimum_numf128"));
    }

    /// §7.3: conversions with an `fp128`, `x86_fp80` or `half` side.
    #[test]
    fn conversion_routines() {
        use FpType::{Double, Float, Fp128, Half, X86Fp80};
        let x86 = Arch::X86_64;
        let a64 = Arch::Aarch64;
        assert_eq!(fp_trunc_routine(Fp128, X86Fp80, x86), Some(RuntimeFn::TruncTfXf2));
        assert_eq!(fp_ext_routine(X86Fp80, Fp128, x86), Some(RuntimeFn::ExtendXfTf2));
        assert_eq!(fp_ext_routine(Half, Float, x86), Some(RuntimeFn::ExtendHfSf2));
        assert_eq!(fp_trunc_routine(Float, Half, x86), Some(RuntimeFn::TruncSfHf2));
        assert_eq!(fp_ext_routine(Half, Float, a64), None);
        assert_eq!(fp_ext_routine(Float, Fp128, a64), Some(RuntimeFn::ExtendSfTf2));
        assert_eq!(fp_trunc_routine(Fp128, Double, a64), Some(RuntimeFn::TruncTfDf2));
        assert_eq!(int_to_fp_routine(true, 128, X86Fp80, x86), Some(RuntimeFn::FloatTiXf));
        assert_eq!(fp_to_int_routine(X86Fp80, true, 128, x86), Some(RuntimeFn::FixXfTi));
        assert_eq!(fp_to_int_routine(X86Fp80, true, 64, x86), None);
        assert_eq!(fp_to_int_routine(Fp128, false, 17, a64), Some(RuntimeFn::FixunsTfSi));
        assert_eq!(fp_to_int_routine(Fp128, true, 33, a64), Some(RuntimeFn::FixTfDi));
        assert_eq!(fp_to_int_routine(Fp128, true, 129, a64), None);
        assert_eq!(int_to_fp_routine(false, 64, Fp128, a64), Some(RuntimeFn::FloatunDiTf));
        assert_eq!(int_to_fp_routine(true, 100, Half, a64), Some(RuntimeFn::FloatTiSf));
        assert_eq!(int_to_fp_routine(true, 100, Half, x86), Some(RuntimeFn::FloatTiHf));
        assert_eq!(int_to_fp_routine(true, 64, Double, x86), None);
    }

    /// §7.3: the `i128` division routines, alike on both targets.
    #[test]
    fn divide_routines() {
        let name = |op| int_divide_routine(op).map(RuntimeFn::name);
        assert_eq!(name(Opcode::UDiv), Some("__udivti3"));
        assert_eq!(name(Opcode::SDiv), Some("__divti3"));
        assert_eq!(name(Opcode::URem), Some("__umodti3"));
        assert_eq!(name(Opcode::SRem), Some("__modti3"));
        assert_eq!(name(Opcode::Add), None);
    }

    /// §7.3: `fcmp fp128` relies only on the signs of four routines.
    #[test]
    fn fp128_compares() {
        use FloatPredicate as P;
        let c = |p| fp128_compare(p).map(|c| (c.func.name(), c.swap, c.result));
        assert_eq!(c(P::Ogt), Some(("__lttf2", true, ZeroCmp::Lt)));
        assert_eq!(c(P::Uno), Some(("__unordtf2", false, ZeroCmp::Ne)));
        assert_eq!(c(P::Ueq), None);
        assert_eq!(c(P::One), None);
        assert_eq!(c(P::True), None);
    }
}
