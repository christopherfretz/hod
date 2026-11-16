//! 32-bit x86 CPUs and their features (`spec/ir.md` §2.1, WP-286, design
//! 0008 §1.9 and §5.1).
//!
//! On 32-bit x86 the CPU decides what code may use: clang writes
//! `"target-cpu"="pentium4"` with a `target-features` list that does not
//! name `sse2`, and `llc` uses SSE2 because the CPU has it. So the
//! features of a function are those of its CPU, then its own
//! `target-features` items, then `-mattr`'s, each `+name` or `-name`
//! with LLVM's implications (`+avx` turns on `sse4.2` and what it
//! implies; `-sse2` turns off `sse3` and what implies it).
//!
//! [`X86Features`] holds the features that change the code or the calling
//! convention of a 32-bit function: the x87 unit, `cmov`, `cmpxchg8b`, the
//! SSE levels (scalar `float` in SSE, scalar `double` in SSE2, vectors in
//! `xmm`), AVX and AVX-512 (256- and 512-bit vectors in `ymm` and `zmm`),
//! FMA and `popcnt`. Every other LLVM feature name leaves the set as it is.
//!
//! [`I386_CPUS`] is every CPU name LLVM 22's `llc -mtriple=i686-linux-gnu
//! -mcpu=help` lists (LLVM 19 lists the same less `diamondrapids`,
//! `novalake` and `wildcatlake`), with the features `llc` 19 and 22 show
//! for it in code (`crates/hod_ir/tests/x86_cpus.rs`: `cmov` for a
//! `select`, `cmpxchg8b` for a 64-bit `cmpxchg`, `mulss`/`mulsd` for scalar
//! multiplies, `ymm`/`zmm` for wide vector arguments, ...). A CPU that is
//! not in the table is not known ([`UnknownCpu`]). A function with no
//! `target-cpu` and no `-mcpu` is compiled for `generic` (x87 and
//! `cmpxchg8b`, whatever the triple's `i386` to `i686`); clang's default
//! is `"target-cpu"="i686"` (`cmov`, `cx8`, x87, no SSE).

use std::fmt;

/// A set of the 32-bit x86 features hod's code depends on.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct X86Features(u16);

impl X86Features {
    /// No feature.
    pub const NONE: X86Features = X86Features(0);
    /// The x87 floating-point unit (`x87`).
    pub const X87: X86Features = X86Features(1 << 0);
    /// Conditional moves (`cmov`).
    pub const CMOV: X86Features = X86Features(1 << 1);
    /// `cmpxchg8b` (`cx8`).
    pub const CX8: X86Features = X86Features(1 << 2);
    /// SSE: `xmm` registers, scalar and vector `float` (`sse`).
    pub const SSE: X86Features = X86Features(1 << 3);
    /// SSE2: scalar and vector `double`, integer vectors (`sse2`).
    pub const SSE2: X86Features = X86Features(1 << 4);
    /// `sse3`.
    pub const SSE3: X86Features = X86Features(1 << 5);
    /// `ssse3`.
    pub const SSSE3: X86Features = X86Features(1 << 6);
    /// `sse4.1`.
    pub const SSE4_1: X86Features = X86Features(1 << 7);
    /// `sse4.2`.
    pub const SSE4_2: X86Features = X86Features(1 << 8);
    /// `popcnt`.
    pub const POPCNT: X86Features = X86Features(1 << 9);
    /// AVX: `ymm` registers (`avx`).
    pub const AVX: X86Features = X86Features(1 << 10);
    /// `avx2`.
    pub const AVX2: X86Features = X86Features(1 << 11);
    /// Fused multiply-add, three operands (`fma`).
    pub const FMA: X86Features = X86Features(1 << 12);
    /// AVX-512 foundation: `zmm` registers (`avx512f`).
    pub const AVX512F: X86Features = X86Features(1 << 13);

    /// Each feature with its LLVM name, in the order of the bits.
    pub const NAMES: [(&'static str, X86Features); 14] = [
        ("x87", X86Features::X87),
        ("cmov", X86Features::CMOV),
        ("cx8", X86Features::CX8),
        ("sse", X86Features::SSE),
        ("sse2", X86Features::SSE2),
        ("sse3", X86Features::SSE3),
        ("ssse3", X86Features::SSSE3),
        ("sse4.1", X86Features::SSE4_1),
        ("sse4.2", X86Features::SSE4_2),
        ("popcnt", X86Features::POPCNT),
        ("avx", X86Features::AVX),
        ("avx2", X86Features::AVX2),
        ("fma", X86Features::FMA),
        ("avx512f", X86Features::AVX512F),
    ];

    /// Both sets.
    #[must_use]
    pub const fn union(self, other: X86Features) -> X86Features {
        X86Features(self.0 | other.0)
    }

    /// Whether every feature of `other` is in `self`.
    pub const fn has(self, other: X86Features) -> bool {
        self.0 & other.0 == other.0
    }

    /// The bits.
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// The feature named `name` (an LLVM name of [`NAMES`](Self::NAMES)).
    pub fn named(name: &str) -> Option<X86Features> {
        Self::NAMES.iter().find(|&&(n, _)| n == name).map(|&(_, f)| f)
    }

    /// `feature` and every feature it implies (LLVM's `X86.td`): each SSE
    /// level the one below it, `avx` `sse4.2`, `avx2` and `fma` `avx`,
    /// `avx512f` `avx2` and `fma`.
    const fn implied(feature: X86Features) -> X86Features {
        let mut set = feature.0;
        let chain = [
            (X86Features::AVX512F, X86Features::AVX2.0 | X86Features::FMA.0),
            (X86Features::AVX2, X86Features::AVX.0),
            (X86Features::FMA, X86Features::AVX.0),
            (X86Features::AVX, X86Features::SSE4_2.0),
            (X86Features::SSE4_2, X86Features::SSE4_1.0),
            (X86Features::SSE4_1, X86Features::SSSE3.0),
            (X86Features::SSSE3, X86Features::SSE3.0),
            (X86Features::SSE3, X86Features::SSE2.0),
            (X86Features::SSE2, X86Features::SSE.0),
        ];
        let mut at = 0;
        while at < chain.len() {
            if set & chain[at].0.0 != 0 {
                set |= chain[at].1;
            }
            at += 1;
        }
        X86Features(set)
    }

    /// The set after one `target-features` or `-mattr` item: `+name` adds
    /// the feature and what it implies, `-name` removes it and every
    /// feature that implies it. An item that names no feature of the set
    /// (`+fxsr`, `-avx512bw`, `+soft-float`, ...) or has no sign changes
    /// nothing: what such a feature means for the code is the code
    /// generator's to judge.
    #[must_use]
    pub fn apply(self, item: &str) -> X86Features {
        let (enable, name) = match item.as_bytes().first() {
            Some(b'+') => (true, &item[1..]),
            Some(b'-') => (false, &item[1..]),
            _ => return self,
        };
        let Some(feature) = X86Features::named(name) else { return self };
        if enable {
            return self.union(X86Features::implied(feature)).with_cmov_of_sse();
        }
        let mut set = self.0;
        for (_, other) in X86Features::NAMES {
            if X86Features::implied(other).has(feature) {
                set &= !other.0;
            }
        }
        X86Features(set).with_cmov_of_sse()
    }

    /// LLVM uses `cmov` on every CPU with SSE, whether or not `cmov` is
    /// among its features (`X86Subtarget::canUseCMOV`; `llc` with
    /// `-mattr=+sse2` and no CPU, or with `-cmov`, writes `cmov`).
    const fn with_cmov_of_sse(self) -> X86Features {
        if self.has(X86Features::SSE) { self.union(X86Features::CMOV) } else { self }
    }

    /// The set after a comma-separated list of items ([`apply`](Self::apply)).
    #[must_use]
    pub fn apply_list(self, list: &str) -> X86Features {
        list.split(',').fold(self, X86Features::apply)
    }

    /// The features of `cpu`, an LLVM 32-bit x86 CPU name; `None` for a
    /// name LLVM does not list.
    pub fn of_cpu(cpu: &str) -> Option<X86Features> {
        I386_CPUS.binary_search_by(|&(name, _)| name.cmp(cpu)).ok().map(|at| I386_CPUS[at].1)
    }

    /// The features a function is compiled with, as `llc` decides them:
    /// the CPU is the function's `target-cpu`, else `-mcpu`, else
    /// `generic`; then the function's `target-features` items, then
    /// `-mattr`'s, in order.
    ///
    /// # Errors
    /// [`UnknownCpu`] when the CPU is not in [`I386_CPUS`].
    pub fn of_function(
        target_cpu: Option<&str>,
        target_features: Option<&str>,
        mcpu: Option<&str>,
        mattr: &[String],
    ) -> Result<X86Features, UnknownCpu> {
        let cpu = target_cpu.or(mcpu).unwrap_or("generic");
        let base = X86Features::of_cpu(cpu).ok_or_else(|| UnknownCpu(cpu.to_owned()))?;
        let own = target_features.map_or(base, |list| base.apply_list(list));
        Ok(mattr.iter().fold(own, |set, item| set.apply_list(item)))
    }
}

impl fmt::Debug for X86Features {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> =
            X86Features::NAMES.iter().filter(|&&(_, x)| self.has(x)).map(|&(n, _)| n).collect();
        write!(f, "X86Features({})", names.join(","))
    }
}

/// A `target-cpu` or `-mcpu` name that is no 32-bit x86 CPU LLVM knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownCpu(pub String);

impl fmt::Display for UnknownCpu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown 32-bit x86 CPU `{}`", self.0)
    }
}

/// `i386`, `i486`, `c3`, the `winchip` CPUs: the x87 unit only.
const I386: X86Features = X86Features::X87;
/// `generic`, `i586`, `pentium`, the K6s, `geode`: and `cmpxchg8b`.
const PENTIUM: X86Features = I386.union(X86Features::CX8);
/// `lakemont` (Quark): `cmpxchg8b`, no x87 unit.
const LAKEMONT: X86Features = X86Features::CX8;
/// `i686` and the P6 family: and `cmov`.
const P6: X86Features = PENTIUM.union(X86Features::CMOV);
/// `pentium3`, `athlon-xp`: and SSE.
const P3: X86Features = P6.union(X86Features::SSE);
/// `pentium4`, `pentium-m`, `k8`, `x86-64`: and SSE2.
const P4: X86Features = P3.union(X86Features::SSE2);
/// `prescott`, `yonah`, `k8-sse3`: and SSE3.
const PRESCOTT: X86Features = P4.union(X86Features::SSE3);
/// `amdfam10`, `barcelona`: SSE3 and `popcnt`.
const BARCELONA: X86Features = PRESCOTT.union(X86Features::POPCNT);
/// `core2`, `atom`: and SSSE3.
const CORE2: X86Features = PRESCOTT.union(X86Features::SSSE3);
/// `btver1`: SSSE3 and `popcnt`.
const BTVER1: X86Features = CORE2.union(X86Features::POPCNT);
/// `penryn`: and SSE4.1.
const PENRYN: X86Features = CORE2.union(X86Features::SSE4_1);
/// `nehalem` to `westmere`, `silvermont`, `goldmont`, `x86-64-v2`: and
/// SSE4.2 and `popcnt`.
const NEHALEM: X86Features = PENRYN.union(X86Features::SSE4_2).union(X86Features::POPCNT);
/// `sandybridge`, `ivybridge`, `bdver1`, `btver2`: and AVX.
const SANDYBRIDGE: X86Features = NEHALEM.union(X86Features::AVX);
/// `bdver2`, `bdver3`: AVX and FMA, no AVX2.
const BDVER2: X86Features = SANDYBRIDGE.union(X86Features::FMA);
/// `haswell` to the newest cores without AVX-512, `znver1` to `znver3`,
/// `x86-64-v3`: and AVX2 and FMA.
const HASWELL: X86Features = BDVER2.union(X86Features::AVX2);
/// `skx` and the AVX-512 cores, `znver4`, `x86-64-v4`: and AVX-512.
const SKX: X86Features = HASWELL.union(X86Features::AVX512F);

/// Every CPU name of LLVM 22's 32-bit x86 target, sorted by name, with its
/// features (module documentation).
pub const I386_CPUS: [(&str, X86Features); 131] = [
    ("alderlake", HASWELL),
    ("amdfam10", BARCELONA),
    ("arrowlake", HASWELL),
    ("arrowlake-s", HASWELL),
    ("arrowlake_s", HASWELL),
    ("athlon", P6),
    ("athlon-4", P3),
    ("athlon-fx", P4),
    ("athlon-mp", P3),
    ("athlon-tbird", P6),
    ("athlon-xp", P3),
    ("athlon64", P4),
    ("athlon64-sse3", PRESCOTT),
    ("atom", CORE2),
    ("atom_sse4_2", NEHALEM),
    ("atom_sse4_2_movbe", NEHALEM),
    ("barcelona", BARCELONA),
    ("bdver1", SANDYBRIDGE),
    ("bdver2", BDVER2),
    ("bdver3", BDVER2),
    ("bdver4", HASWELL),
    ("bonnell", CORE2),
    ("broadwell", HASWELL),
    ("btver1", BTVER1),
    ("btver2", SANDYBRIDGE),
    ("c3", I386),
    ("c3-2", P3),
    ("cannonlake", SKX),
    ("cascadelake", SKX),
    ("clearwaterforest", HASWELL),
    ("cooperlake", SKX),
    ("core-avx-i", SANDYBRIDGE),
    ("core-avx2", HASWELL),
    ("core2", CORE2),
    ("core_2_duo_sse4_1", PENRYN),
    ("core_2_duo_ssse3", CORE2),
    ("core_2nd_gen_avx", SANDYBRIDGE),
    ("core_3rd_gen_avx", SANDYBRIDGE),
    ("core_4th_gen_avx", HASWELL),
    ("core_4th_gen_avx_tsx", HASWELL),
    ("core_5th_gen_avx", HASWELL),
    ("core_5th_gen_avx_tsx", HASWELL),
    ("core_aes_pclmulqdq", NEHALEM),
    ("core_i7_sse4_2", NEHALEM),
    ("corei7", NEHALEM),
    ("corei7-avx", SANDYBRIDGE),
    ("diamondrapids", SKX),
    ("emeraldrapids", SKX),
    ("generic", PENTIUM),
    ("geode", PENTIUM),
    ("goldmont", NEHALEM),
    ("goldmont-plus", NEHALEM),
    ("goldmont_plus", NEHALEM),
    ("gracemont", HASWELL),
    ("grandridge", HASWELL),
    ("graniterapids", SKX),
    ("graniterapids-d", SKX),
    ("graniterapids_d", SKX),
    ("haswell", HASWELL),
    ("i386", I386),
    ("i486", I386),
    ("i586", PENTIUM),
    ("i686", P6),
    ("icelake-client", SKX),
    ("icelake-server", SKX),
    ("icelake_client", SKX),
    ("icelake_server", SKX),
    ("ivybridge", SANDYBRIDGE),
    ("k6", PENTIUM),
    ("k6-2", PENTIUM),
    ("k6-3", PENTIUM),
    ("k8", P4),
    ("k8-sse3", PRESCOTT),
    ("knl", SKX),
    ("knm", SKX),
    ("lakemont", LAKEMONT),
    ("lunarlake", HASWELL),
    ("meteorlake", HASWELL),
    ("mic_avx512", SKX),
    ("nehalem", NEHALEM),
    ("nocona", PRESCOTT),
    ("novalake", SKX),
    ("opteron", P4),
    ("opteron-sse3", PRESCOTT),
    ("pantherlake", HASWELL),
    ("penryn", PENRYN),
    ("pentium", PENTIUM),
    ("pentium-m", P4),
    ("pentium-mmx", PENTIUM),
    ("pentium2", P6),
    ("pentium3", P3),
    ("pentium3m", P3),
    ("pentium4", P4),
    ("pentium4m", P4),
    ("pentium_4", P4),
    ("pentium_4_sse3", PRESCOTT),
    ("pentium_ii", P6),
    ("pentium_iii", P3),
    ("pentium_iii_no_xmm_regs", P3),
    ("pentium_m", P4),
    ("pentium_mmx", PENTIUM),
    ("pentium_pro", P6),
    ("pentiumpro", P6),
    ("prescott", PRESCOTT),
    ("raptorlake", HASWELL),
    ("rocketlake", SKX),
    ("sandybridge", SANDYBRIDGE),
    ("sapphirerapids", SKX),
    ("sierraforest", HASWELL),
    ("silvermont", NEHALEM),
    ("skx", SKX),
    ("skylake", HASWELL),
    ("skylake-avx512", SKX),
    ("skylake_avx512", SKX),
    ("slm", NEHALEM),
    ("tigerlake", SKX),
    ("tremont", NEHALEM),
    ("westmere", NEHALEM),
    ("wildcatlake", HASWELL),
    ("winchip-c6", I386),
    ("winchip2", I386),
    ("x86-64", P4),
    ("x86-64-v2", NEHALEM),
    ("x86-64-v3", HASWELL),
    ("x86-64-v4", SKX),
    ("yonah", PRESCOTT),
    ("znver1", HASWELL),
    ("znver2", HASWELL),
    ("znver3", HASWELL),
    ("znver4", SKX),
    ("znver5", SKX),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &str) -> X86Features {
        names.split(',').filter(|n| !n.is_empty()).fold(X86Features::NONE, |s, n| {
            s.union(X86Features::named(n).unwrap_or_else(|| panic!("{n}")))
        })
    }

    #[test]
    fn table_is_sorted_and_unique() {
        assert!(I386_CPUS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    /// Design 0008 §5.1: clang's default `i686` has `cmov`, `cx8` and x87,
    /// no SSE; `pentium4` has SSE2 though clang's list names only `sse`.
    #[test]
    fn default_builds() {
        let clang_default =
            X86Features::of_function(Some("i686"), Some("+cmov,+cx8,+x87"), None, &[]).unwrap();
        assert_eq!(clang_default, set("x87,cmov,cx8"));
        let p4 = X86Features::of_function(
            Some("pentium4"),
            Some("+cmov,+cx8,+fxsr,+mmx,+sse,+x87"),
            None,
            &[],
        )
        .unwrap();
        assert!(p4.has(X86Features::SSE2) && !p4.has(X86Features::SSE3));
        // No CPU anywhere: `generic`.
        assert_eq!(X86Features::of_function(None, None, None, &[]).unwrap(), set("x87,cx8"));
        assert_eq!(X86Features::of_cpu("i386"), Some(set("x87")));
        assert_eq!(X86Features::of_cpu("lakemont"), Some(set("cx8")));
    }

    /// `target-cpu` before `-mcpu`; `-mattr` after the function's own
    /// features (what `llc` does with both).
    #[test]
    fn precedence() {
        let mattr = ["+sse2".to_owned()];
        let f = X86Features::of_function(Some("i686"), None, Some("pentium4"), &[]).unwrap();
        assert!(!f.has(X86Features::SSE));
        let f = X86Features::of_function(None, None, Some("pentium4"), &[]).unwrap();
        assert!(f.has(X86Features::SSE2));
        let f = X86Features::of_function(Some("i686"), Some("-sse2"), None, &mattr).unwrap();
        assert!(f.has(X86Features::SSE2) && f.has(X86Features::SSE));
        let off = ["-sse".to_owned()];
        let f = X86Features::of_function(Some("pentium4"), Some("+sse2"), None, &off).unwrap();
        assert!(!f.has(X86Features::SSE) && !f.has(X86Features::SSE2));
        assert_eq!(
            X86Features::of_function(Some("pentium5"), None, None, &[]),
            Err(UnknownCpu("pentium5".into()))
        );
        assert_eq!(
            X86Features::of_function(None, None, Some("cortex-a53"), &[]),
            Err(UnknownCpu("cortex-a53".into()))
        );
    }

    /// LLVM's implications, both ways.
    #[test]
    fn implications() {
        let none = X86Features::NONE;
        assert_eq!(none.apply("+avx"), set("cmov,sse,sse2,sse3,ssse3,sse4.1,sse4.2,avx"));
        // `cmov` with SSE, whatever the list says.
        assert!(X86Features::of_cpu("pentium4").unwrap().apply("-cmov").has(X86Features::CMOV));
        assert!(!X86Features::of_cpu("i686").unwrap().apply("-cmov").has(X86Features::CMOV));
        assert_eq!(none.apply("+avx512f"), none.apply("+avx2").apply("+fma").apply("+avx512f"));
        assert!(none.apply("+avx512f").has(set("avx2,fma,avx")));
        let skx = X86Features::of_cpu("skx").unwrap();
        assert_eq!(skx.apply("-sse4.1"), set("x87,cmov,cx8,sse,sse2,sse3,ssse3,popcnt"));
        assert_eq!(skx.apply("-avx"), set("x87,cmov,cx8,sse,sse2,sse3,ssse3,sse4.1,sse4.2,popcnt"));
        assert_eq!(skx.apply("-avx2"), skx.apply("-avx2").apply("-avx512f"));
        assert!(!skx.apply("-avx2").has(X86Features::AVX512F));
        assert!(skx.apply("-avx2").has(X86Features::FMA));
        assert!(!skx.apply("-x87").has(X86Features::X87) && skx.apply("-x87").has(set("sse2")));
        // Names outside the set, and items without a sign, change nothing.
        for item in ["+fxsr", "-mmx", "+avx512bw", "+soft-float", "sse2", "", "+"] {
            assert_eq!(skx.apply(item), skx, "{item}");
            assert_eq!(none.apply(item), none, "{item}");
        }
        assert_eq!(none.apply_list("+sse2,,-sse,+cmov"), set("cmov"));
    }
}
