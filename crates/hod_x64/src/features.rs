//! Target features (`spec/codegen.md` §5.6.2): which `+name`/`-name`
//! items of a `target-features` string or of `-mattr` hod's x86-64 code
//! is correct under.
//!
//! hod emits the base x86-64 instruction set (SSE2, which every x86-64
//! CPU runs) and never chooses an instruction by a feature. So a feature
//! that only makes instructions *available* changes nothing hod emits,
//! enabled or disabled, and is accepted: a module built for any
//! `-march=` CPU clang 19 and 22 and kiln know (`x86-64-v2` to `-v4`,
//! `nehalem` to the newest Intel and AMD cores) is not declined as a
//! whole (design 0006 §10.1, WP-201). `target-cpu` and `tune-cpu` are not
//! features and are accepted with any value, and so are the tuning
//! features (`fast-*`, `slow-*`, `prefer-*`, ...). A feature that changes
//! what the code must keep (no SSE registers, a soft-float or ILP32 ABI,
//! another mode, a mitigation hod does not apply) is refused by name.
//! Everything else is unknown and refused: a feature hod has not judged
//! is not assumed harmless.
//!
//! Each feature of LLVM 19's and 22's x86 list is judged here: [`JUDGED`]
//! (the baseline and the contract features), [`AVAILABLE`] (instructions
//! only) and [`tuning`]. The evidence (WP-201) is `llc`'s own calling
//! convention: for every feature, enabled alone and disabled on the
//! richest CPUs, and for every CPU string of the census, `llc` 19 and 22
//! place the arguments and results of a set of signatures and keep the
//! registers across calls exactly as without features, except where
//! [`of_function`]'s AVX and AVX-512 say otherwise, which the signature's
//! assignment refuses (`tests/features_census.rs`).
//!
//! Two things a function's features decide besides instructions are
//! [`of_function`]'s (`spec/codegen.md` §5.6.9): where a vector wider than
//! 128 bits travels (AVX, `spec/abi-x64.md` §1.3), and whether the
//! function may use the x87 unit (`-x87`, refused only for a function that
//! has an `x86_fp80` value: [`check_function`]).

use crate::abi;

/// `+soft-float`: floating point in general registers, another ABI.
pub const SOFT_FLOAT: &str = "attr:target-feature-soft-float";
/// `-sse` or `-sse2`: no SSE registers, which hod's floating point and
/// the SysV ABI use.
pub const NO_SSE: &str = "attr:target-feature-no-sse";
/// `-x87`: no x87 unit; `x86_fp80` would need a soft implementation.
/// Refused in a function only when it has an `x86_fp80` value (`X64`'s
/// `begin_function`), and always in `-mattr`, which reaches no function.
pub const NO_X87: &str = "attr:target-feature-no-x87";
/// `+retpoline*`: indirect branches through thunks, which hod does not
/// emit.
pub const RETPOLINE: &str = "attr:target-feature-retpoline";
/// `+harden-sls-*`: straight-line-speculation hardening hod does not do.
pub const HARDEN_SLS: &str = "attr:target-feature-harden-sls";
/// `+lvi-*`: load-value-injection hardening hod does not do.
pub const LVI: &str = "attr:target-feature-lvi";
/// `+seses`: a speculation barrier before every load, store and
/// conditional branch (and LVI's control-flow hardening), which hod does
/// not emit.
pub const SESES: &str = "attr:target-feature-seses";
/// `+x32`: the ILP32 programming model (32-bit pointers), another ABI.
pub const X32: &str = "attr:target-feature-x32";
/// `+16bit-mode`, `+32bit-mode`, `-64bit-mode`, `-64bit`: code for
/// another processor mode.
pub const MODE: &str = "attr:target-feature-mode";
/// `+tagged-globals`: a global's address is taken with a sequence that
/// keeps a memory tag in its upper bits.
pub const TAGGED_GLOBALS: &str = "attr:target-feature-tagged-globals";
/// A feature hod has not judged, or an item that is not `+name` or
/// `-name`.
pub const UNKNOWN: &str = "attr:target-feature-unknown";

/// Every reason [`check`] gives.
pub const REASONS: [&str; 11] = [
    SOFT_FLOAT,
    NO_SSE,
    NO_X87,
    RETPOLINE,
    HARDEN_SLS,
    LVI,
    SESES,
    X32,
    MODE,
    TAGGED_GLOBALS,
    UNKNOWN,
];

/// A refused feature: the reason, and the item as written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused<'a> {
    /// The stable name (one of [`REASONS`]).
    pub reason: &'static str,
    /// The item that was refused, sign included.
    pub feature: &'a str,
}

/// What a judged feature does to hod's code, enabled (`+`) and disabled
/// (`-`): `None` where it is accepted.
#[derive(Clone, Copy, Debug)]
struct Judged {
    name: &'static str,
    on: Option<&'static str>,
    off: Option<&'static str>,
}

/// The features whose one sign changes what the code must keep (design
/// 0006 §2.7, §10.1), one line each.
const JUDGED: &[Judged] = &[
    // The x86-64 baseline: what clang and kiln write for every function.
    // Turning one off takes away what SysV passes floats in, or `x86_fp80`.
    Judged { name: "sse", on: None, off: Some(NO_SSE) },
    Judged { name: "sse2", on: None, off: Some(NO_SSE) },
    Judged { name: "x87", on: None, off: Some(NO_X87) },
    // The mode: the triple's, 64-bit.
    Judged { name: "64bit", on: None, off: Some(MODE) },
    Judged { name: "64bit-mode", on: None, off: Some(MODE) },
    Judged { name: "32bit-mode", on: Some(MODE), off: None },
    Judged { name: "16bit-mode", on: Some(MODE), off: None },
    // Contract features: off is what hod's code already keeps.
    Judged { name: "soft-float", on: Some(SOFT_FLOAT), off: None },
    Judged { name: "x32", on: Some(X32), off: None },
    Judged { name: "seses", on: Some(SESES), off: None },
    Judged { name: "tagged-globals", on: Some(TAGGED_GLOBALS), off: None },
];

/// Features that only make instructions (or registers no value of hod's
/// travels in) available, accepted either way: LLVM 19's and 22's x86
/// features but the baseline, the contract and the tuning ones. Notes:
///
/// - `avx`, `avx2`, the AVX-512 and AVX10 families, and what implies them
///   (`fma`, `f16c`, `vaes`, `sha512`, ...): the instructions only. Where
///   a wide vector or a vector of `i1` then travels is the signature's
///   (`x64:wide-vector-abi`, [`of_function`]), and `vzeroupper` is not
///   hod's to write (it never dirties the upper halves).
/// - `cx16`: X-atomic (WP-133) uses `cmpxchg16b` inline with `+cx16` and
///   libatomic's `__atomic_*_16` without; on a CPU with `cmpxchg16b`
///   libatomic uses the same instruction, so either is lock-free and the
///   two halves of a program agree.
/// - APX (`egpr`, `ndd`, `ppx`, `push2pop2`, `ccmp`, `cf`, `nf`, `zu`):
///   `r16`..`r31` are caller-saved and carry no argument; `llc` places
///   every signature and keeps the same registers across calls with them.
/// - `mmx`: no signature changes (LLVM passes `<1 x i64>` and `<2 x i32>`
///   as SSE values either way).
pub const AVAILABLE: &[&str] = &[
    "mmx", "fxsr", "cmov", "cx8", "cx16", "nopl", "sahf", "popcnt", "lzcnt", "bmi", "bmi2", "movbe",
    "crc32", "sse3", "ssse3", "sse4.1", "sse4.2", "sse4a", "avx", "avx2", "fma", "fma4", "f16c",
    "xop", "tbm", "lwp", "adx", "aes", "pclmul", "vaes", "vpclmulqdq", "gfni", "sha", "sha512",
    "sm3", "sm4", "xsave", "xsaveopt", "xsavec", "xsaves", "fsgsbase", "rdrnd", "rdseed", "rdpid",
    "rdpru", "invpcid", "prfchw", "prefetchi", "clflushopt", "clwb", "clzero", "cldemote",
    "movdiri", "movdir64b", "movrs", "mwaitx", "waitpkg", "pconfig", "pku", "ptwrite", "rtm",
    "tsxldtrk", "serialize", "sgx", "shstk", "uintr", "usermsr", "enqcmd", "hreset", "kl",
    "widekl", "wbnoinvd", "cmpccxadd", "raoint", "branch-hint", "evex512", "avx512f",
    "avx512cd", "avx512dq", "avx512bw", "avx512vl", "avx512ifma", "avx512vbmi", "avx512vbmi2",
    "avx512vnni", "avx512bitalg", "avx512vpopcntdq", "avx512bf16", "avx512fp16",
    "avx512vp2intersect", "avx10.1", "avx10.1-256", "avx10.1-512", "avx10.2", "avx10.2-512",
    "avxifma", "avxneconvert", "avxvnni", "avxvnniint8", "avxvnniint16", "amx-tile", "amx-int8",
    "amx-bf16", "amx-fp16", "amx-complex", "amx-fp8", "amx-tf32", "amx-movrs", "amx-avx512",
    "egpr", "ndd", "ppx", "push2pop2", "ccmp", "cf", "nf", "zu",
];

/// Tuning features LLVM 19 and 22 know by name: how `llc` chooses among
/// correct sequences, nothing a caller or callee can tell. Besides these,
/// every name of the families [`tuning`] lists.
const TUNING: &[&str] = &[
    "allow-light-256-bit",
    "branchfusion",
    "ermsb",
    "faster-shift-than-shuffle",
    "fsrm",
    "idivl-to-divb",
    "idivq-to-divl",
    "inline-asm-use-gpr32",
    "lea-sp",
    "lea-uses-ag",
    "macrofusion",
    "pad-short-functions",
    "sbb-dep-breaking",
    "sse-unaligned-mem",
    "use-glm-div-sqrt-costs",
    "use-slm-arith-costs",
    "vzeroupper",
];

/// Whether `name` is a tuning feature: one of [`TUNING`], or of a family
/// LLVM names only tuning features with (`fast-*`, `slow-*`, `prefer-*`,
/// `false-deps-*`, `tuning-*`, `no-bypass-delay*`).
pub fn tuning(name: &str) -> bool {
    TUNING.contains(&name)
        || ["fast-", "slow-", "prefer-", "false-deps-", "tuning-", "no-bypass-delay"]
            .iter()
            .any(|family| name.starts_with(family))
}

/// Checks one feature string: items separated by `,`, each `+name` or
/// `-name`; empty items are skipped (`-mattr` drops them, and LLVM's
/// parser does). `-x87` is refused ([`NO_X87`]): this is the judgement of
/// `-mattr`; a function's own string is [`check_function`]'s.
///
/// # Errors
/// The first item hod's code is not correct under, with its reason.
pub fn check(features: &str) -> Result<(), Refused<'_>> {
    features.split(',').filter(|item| !item.is_empty()).try_for_each(check_one)
}

/// [`check`] for a function's `target-features`: `-x87` is accepted here,
/// and refused as [`NO_X87`] by code generation only when the function
/// has an `x86_fp80` value ([`FnFeatures::no_x87`]).
///
/// # Errors
/// As [`check`].
pub fn check_function(features: &str) -> Result<(), Refused<'_>> {
    features.split(',').filter(|item| !item.is_empty() && *item != "-x87").try_for_each(check_one)
}

/// What a function's `target-cpu` and `target-features` decide besides
/// the instructions LLVM may choose (`spec/codegen.md` §5.6.9).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct FnFeatures {
    /// Whether AVX and AVX-512 may be on: what `abi::assign` is given. A
    /// feature hod cannot rule out counts as on, which refuses the
    /// signatures it would change (`x64:wide-vector-abi`).
    pub abi: abi::Features,
    /// `-x87` is in force.
    pub no_x87: bool,
}

/// CPUs (`target-cpu`, LLVM's names) without AVX: `llc` 19 and 22 pass a
/// `<4 x double>` in two `xmm` for each.
const CPUS_WITHOUT_AVX: &[&str] = &[
    "",
    "generic",
    "x86-64",
    "x86-64-v2",
    "nocona",
    "prescott",
    "core2",
    "penryn",
    "nehalem",
    "corei7",
    "westmere",
    "bonnell",
    "atom",
    "silvermont",
    "slm",
    "goldmont",
    "goldmont-plus",
    "tremont",
    "k8",
    "k8-sse3",
    "opteron",
    "opteron-sse3",
    "athlon64",
    "athlon64-sse3",
    "athlon-fx",
    "amdfam10",
    "barcelona",
    "btver1",
];

/// CPUs with AVX and without AVX-512: `ymm` for a `<4 x double>`. Every
/// other CPU, known to LLVM or not, may have AVX-512.
const CPUS_WITHOUT_AVX512: &[&str] = &[
    "x86-64-v3",
    "sandybridge",
    "corei7-avx",
    "ivybridge",
    "core-avx-i",
    "haswell",
    "core-avx2",
    "broadwell",
    "skylake",
    "alderlake",
    "raptorlake",
    "meteorlake",
    "gracemont",
    "arrowlake",
    "arrowlake-s",
    "wildcatlake",
    "lunarlake",
    "pantherlake",
    "sierraforest",
    "grandridge",
    "clearwaterforest",
    "btver2",
    "bdver1",
    "bdver2",
    "bdver3",
    "bdver4",
    "znver1",
    "znver2",
    "znver3",
];

/// What a function compiled with `target-cpu` `cpu` and `target-features`
/// `features` (either absent) may assume, as `llc` reads them: the CPU's
/// features, then each item in order, `+name` turning on `name` and what
/// it implies, `-name` turning off `name` and what implies it
/// (`spec/codegen.md` §5.6.9). The caller's features decide every
/// signature it meets, its own and each call's, a direct call to a
/// function with other features included (`spec/abi-x64.md` §1.3).
pub fn of_function(cpu: Option<&str>, features: Option<&str>) -> FnFeatures {
    let cpu = cpu.unwrap_or("");
    let (mut avx, mut avx512) = if CPUS_WITHOUT_AVX.contains(&cpu) {
        (false, false)
    } else {
        (true, !CPUS_WITHOUT_AVX512.contains(&cpu))
    };
    let mut no_x87 = false;
    for item in features.unwrap_or("").split(',') {
        let (on, name) = match item.split_at_checked(1) {
            Some(("+", name)) => (true, name),
            Some(("-", name)) => (false, name),
            _ => continue,
        };
        // `amx-avx512` does not imply AVX-512 in `llc` 22, but its name
        // says it may: an over-approximation only refuses.
        let is_avx512 =
            name.starts_with("avx512") || name.starts_with("avx10") || name == "amx-avx512";
        match (on, name) {
            (_, "x87") => no_x87 = !on,
            // What implies AVX-512, and AVX-512 itself.
            (true, _) if is_avx512 => (avx, avx512) = (true, true),
            // What implies AVX (`llc` 19 and 22 place a `<8 x float>` in
            // `ymm0` under each, WP-201).
            (
                true,
                "avx" | "avx2" | "fma" | "f16c" | "fma4" | "xop" | "vaes" | "vpclmulqdq" | "sha512"
                | "sm3" | "sm4",
            ) => {
                avx = true;
            }
            (true, _) if name.starts_with("avx") => avx = true,
            // What AVX implies.
            (false, "sse" | "sse2" | "sse3" | "ssse3" | "sse4.1" | "sse4.2" | "avx") => {
                (avx, avx512) = (false, false);
            }
            // What AVX-512 implies.
            (false, "avx2" | "fma" | "f16c" | "avx512f") => avx512 = false,
            _ => {}
        }
    }
    FnFeatures { abi: abi::Features { avx, avx512 }, no_x87 }
}

/// One item of a feature string.
///
/// # Errors
/// As [`check`].
pub fn check_one(item: &str) -> Result<(), Refused<'_>> {
    let refuse = |reason| Err(Refused { reason, feature: item });
    let (on, name) = match item.split_at_checked(1) {
        Some(("+", name)) => (true, name),
        Some(("-", name)) => (false, name),
        _ => return refuse(UNKNOWN),
    };
    if let Some(judged) = JUDGED.iter().find(|judged| judged.name == name) {
        return match if on { judged.on } else { judged.off } {
            Some(reason) => refuse(reason),
            None => Ok(()),
        };
    }
    if AVAILABLE.contains(&name) || tuning(name) {
        return Ok(());
    }
    match mitigation(name) {
        // Off is what hod's code already keeps.
        Some(_) if !on => Ok(()),
        Some(reason) => refuse(reason),
        None => refuse(UNKNOWN),
    }
}

/// The reason of a family of mitigation features, if `name` is one.
fn mitigation(name: &str) -> Option<&'static str> {
    Some(match name {
        _ if name.starts_with("retpoline") => RETPOLINE,
        _ if name.starts_with("harden-sls-") => HARDEN_SLS,
        _ if name.starts_with("lvi-") => LVI,
        _ => return None,
    })
}

/// Whether `ty` is or contains an `x86_fp80` (a function type: in its
/// return or a parameter).
pub fn has_x86_fp80(types: &hod_ir::TypeTable, ty: hod_ir::TypeId) -> bool {
    use hod_ir::TypeKind;
    match types.kind(ty) {
        TypeKind::X86Fp80 => true,
        TypeKind::Struct { fields, .. } => fields.iter().any(|&f| has_x86_fp80(types, f)),
        TypeKind::Array { elem, .. } | TypeKind::Vector { elem, .. } => has_x86_fp80(types, elem),
        TypeKind::Function { ret, params, .. } => {
            has_x86_fp80(types, ret) || params.iter().any(|&p| has_x86_fp80(types, p))
        }
        _ => false,
    }
}

/// Whether the function `body` has an `x86_fp80` value (§5.6.9): in its
/// signature, or as the result or an operand of an instruction, at any
/// depth of an aggregate. A slot of that type that is only addressed is
/// memory, not a value.
pub fn uses_x86_fp80(module: &hod_ir::Module, body: &hod_ir::Body) -> bool {
    let types = &module.types;
    has_x86_fp80(types, body.fn_type())
        || body.insts().any(|inst| {
            has_x86_fp80(types, inst.ty())
                || inst.operands().iter().any(|&v| has_x86_fp80(types, body.value_type(module, v)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(features: &str) -> Option<&'static str> {
        check(features).err().map(|refused| refused.reason)
    }

    /// §5.6.2: what kiln and clang write is accepted, `-march=x86-64-v3`
    /// included.
    #[test]
    fn what_compilers_write_is_accepted() {
        for features in [
            "",
            "+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87",
            "+cmov,+crc32,+cx16,+cx8,+fxsr,+mmx,+popcnt,+sahf,+sse,+sse2,+sse3,+sse4.1,+sse4.2,\
             +ssse3,+x87",
            "+avx,+avx2,+bmi,+bmi2,+cmov,+crc32,+cx16,+cx8,+f16c,+fma,+fxsr,+lzcnt,+mmx,+movbe,\
             +popcnt,+sahf,+sse,+sse2,+sse3,+sse4.1,+sse4.2,+ssse3,+x87,+xsave",
            "-avx,-avx2,-popcnt,-soft-float,-retpoline-indirect-calls,-harden-sls-ret",
            ",,+sse2,",
            // `-march=haswell` (clang 19 and 22, kiln), WP-201.
            "+avx,+avx2,+bmi,+bmi2,+cmov,+crc32,+cx16,+cx8,+f16c,+fma,+fsgsbase,+fxsr,+invpcid,\
             +lzcnt,+mmx,+movbe,+pclmul,+popcnt,+rdrnd,+sahf,+sse,+sse2,+sse3,+sse4.1,+sse4.2,\
             +ssse3,+x87,+xsave,+xsaveopt",
            // AVX-512, APX, AMX: the instructions; tuning names.
            "+avx512f,+avx512bw,+evex512,+avx10.2,+egpr,+ndd,+ppx,+push2pop2,+amx-tile,+amx-avx512",
            "+prefer-256-bit,+fast-gather,+slow-3ops-lea,+false-deps-popcnt,+tuning-fast-imm-vector-shift,\
             -vzeroupper,+no-bypass-delay-mov,+64bit,+64bit-mode,-32bit-mode,-x32,-seses",
        ] {
            assert_eq!(check(features), Ok(()), "{features}");
        }
    }

    /// §5.6.2: what changes the contract, each by its name.
    #[test]
    fn contract_features_are_refused_by_name() {
        for (features, expected) in [
            ("+soft-float", SOFT_FLOAT),
            ("-sse", NO_SSE),
            ("+sse,-sse2", NO_SSE),
            ("-x87", NO_X87),
            ("+retpoline-indirect-calls", RETPOLINE),
            ("+retpoline-external-thunk", RETPOLINE),
            ("+harden-sls-ret", HARDEN_SLS),
            ("+lvi-cfi", LVI),
            ("+lvi-load-hardening", LVI),
            ("+seses", SESES),
            ("+x32", X32),
            ("+32bit-mode", MODE),
            ("+16bit-mode", MODE),
            ("-64bit-mode", MODE),
            ("-64bit", MODE),
            ("+tagged-globals", TAGGED_GLOBALS),
        ] {
            assert_eq!(reason(features), Some(expected), "{features}");
        }
    }

    /// §5.6.2: unknown is refused, with the item.
    #[test]
    fn unknown_features_are_refused() {
        assert_eq!(
            check("+sse2,+avx512f,+frobnicate,+soft-float"),
            Err(Refused { reason: UNKNOWN, feature: "+frobnicate" })
        );
        // A name of a tuning family is a tuning feature; another prefix is
        // not.
        assert_eq!(reason("+fastest"), Some(UNKNOWN));
        assert_eq!(reason("+3dnow"), Some(UNKNOWN));
        assert_eq!(check("sse2"), Err(Refused { reason: UNKNOWN, feature: "sse2" }));
        assert_eq!(reason("-no-such-feature"), Some(UNKNOWN));
        assert_eq!(reason("+"), Some(UNKNOWN));
        assert_eq!(reason("+SSE2"), Some(UNKNOWN));
        // An AArch64 feature is not an x86-64 one.
        assert_eq!(reason("+neon"), Some(UNKNOWN));
    }

    /// §5.6.9: `-x87` in a function's string is left to code generation;
    /// in `-mattr` it stays refused.
    #[test]
    fn no_x87_is_judged_per_function() {
        assert_eq!(check_function("+sse2,-x87"), Ok(()));
        assert_eq!(reason("-x87"), Some(NO_X87));
        assert_eq!(check_function("-x87,+soft-float").map_err(|r| r.reason), Err(SOFT_FLOAT));
        assert!(of_function(None, Some("+sse2,-x87")).no_x87);
        assert!(!of_function(None, Some("-x87,+x87")).no_x87);
        assert!(!of_function(None, None).no_x87);
    }

    /// §5.6.9: the CPU, then the items in order, with their implications.
    #[test]
    fn features_that_decide_the_convention() {
        let abi = |cpu: Option<&str>, features: &str| {
            let f = of_function(cpu, Some(features)).abi;
            (f.avx, f.avx512)
        };
        let base = Some("x86-64");
        assert_eq!(abi(base, "+sse2,+x87"), (false, false));
        assert_eq!(abi(None, ""), (false, false));
        assert_eq!(abi(base, "+avx"), (true, false));
        assert_eq!(abi(base, "+fma"), (true, false));
        assert_eq!(abi(base, "+f16c"), (true, false));
        assert_eq!(abi(base, "+avx2,-avx"), (false, false));
        assert_eq!(abi(base, "-avx,+avx2"), (true, false));
        assert_eq!(abi(base, "+avx,-sse4.2"), (false, false));
        assert_eq!(abi(base, "+avx512f"), (true, true));
        assert_eq!(abi(base, "+avx512f,-avx2"), (true, false));
        // WP-201: what implies AVX without the name.
        assert_eq!(abi(base, "+sha512"), (true, false));
        assert_eq!(abi(base, "+sm3"), (true, false));
        assert_eq!(abi(base, "+sm4"), (true, false));
        assert_eq!(abi(base, "+egpr,+amx-tile,+gfni,+cx16"), (false, false));
        assert_eq!(abi(base, "+amx-avx512"), (true, true));
        assert_eq!(abi(Some("arrowlake-s"), ""), (true, false));
        assert_eq!(abi(Some("opteron"), ""), (false, false));
        assert_eq!(abi(Some("haswell"), ""), (true, false));
        assert_eq!(abi(Some("haswell"), "-avx"), (false, false));
        assert_eq!(abi(Some("skylake-avx512"), ""), (true, true));
        // A CPU hod does not know may have anything.
        assert_eq!(abi(Some("future-cpu"), ""), (true, true));
        assert_eq!(abi(Some("future-cpu"), "-avx512f"), (true, false));
    }

    #[test]
    fn reasons_are_distinct_and_named_like_rows() {
        for (i, name) in REASONS.iter().enumerate() {
            assert!(name.starts_with("attr:target-feature-"), "{name}");
            assert!(!REASONS[..i].contains(name), "{name}");
        }
        for (i, judged) in JUDGED.iter().enumerate() {
            assert!(!JUDGED[..i].iter().any(|j| j.name == judged.name), "{}", judged.name);
            assert!(!AVAILABLE.contains(&judged.name) && !tuning(judged.name), "{}", judged.name);
        }
        for (i, name) in AVAILABLE.iter().enumerate() {
            assert!(!AVAILABLE[..i].contains(name), "{name}");
            assert!(!tuning(name) && mitigation(name).is_none(), "{name}");
        }
    }
}
