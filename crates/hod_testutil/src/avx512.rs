//! Whether, and how, a test executes a program that needs AVX-512 (or any
//! x86-64 feature only some processors have): `spec/harness.md` "CI",
//! "AVX-512 execution", and `docs/design/0007-avx512.md` §9.
//!
//! - `HOD_X64_AVX512_RUNNER` set: the command it names runs every such
//!   program (its words are a command prefix, the program's path is
//!   appended; `native` runs the program directly). It wins over
//!   everything below.
//! - Unset, on x86-64 Linux: the processor's flags are read from
//!   `/proc/cpuinfo`, and a program runs natively when the processor has
//!   every feature it requires ([`CpuInfo::missing`]); otherwise it is
//!   skipped, naming the missing features.
//! - Unset, on any other host: skipped, as before there was detection.
//! - `HOD_AVX512_CPUINFO=<file>` (for testing this plumbing only): the
//!   flags are read from that file instead, on any host, and a program the
//!   flags would let run natively is NOT run: it is reported as a dry run,
//!   since the flags are not the processor's.
//!
//! Every decision is visible: with `HOD_AVX512_REPORT=<file>`, one line per
//! program is appended to the file ([`report`]), `program`, outcome
//! (`executed`, `skipped`, `dry-run`, `failed`) and detail separated by
//! tabs; the hosted x86-64 lane summarizes it. With `HOD_REQUIRE_AVX512=1`
//! a skip or a dry run is a failure ([`skip`]); the hosted lane does not
//! set it, since its runners are not all AVX-512 processors.

use std::collections::BTreeSet;
use std::io::Write as _;

/// A processor's model name and flags, as `/proc/cpuinfo` lists them for
/// its first processor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuInfo {
    pub model: String,
    /// The flags, normalized by [`normalize`].
    pub flags: BTreeSet<String>,
}

/// A feature or flag name with `_`, `-` and `.` removed, lower case, and
/// LLVM's `bmi` spelled as the kernel's `bmi1`: LLVM's `avx512vbmi2` and
/// the kernel's `avx512_vbmi2` are then the same name.
pub fn normalize(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !matches!(c, '_' | '-' | '.'))
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if name == "bmi" { "bmi1".into() } else { name }
}

impl CpuInfo {
    /// Parses the text of `/proc/cpuinfo`: the `model name` and `flags`
    /// lines of the first processor. `None` without a `flags` line (an
    /// AArch64 kernel's lists `Features`) or with an empty one.
    pub fn parse(text: &str) -> Option<CpuInfo> {
        let mut model = None;
        let mut flags = None;
        for line in text.lines() {
            // The first processor ends at the first blank line.
            if line.trim().is_empty() && (model.is_some() || flags.is_some()) {
                break;
            }
            let Some((key, value)) = line.split_once(':') else { continue };
            match key.trim() {
                "model name" if model.is_none() => model = Some(value.trim().to_owned()),
                "flags" if flags.is_none() => {
                    let set: BTreeSet<String> = value.split_whitespace().map(normalize).collect();
                    flags = Some(set);
                }
                _ => {}
            }
        }
        let flags = flags.filter(|f| !f.is_empty())?;
        Some(CpuInfo { model: model.unwrap_or_else(|| "unknown".into()), flags })
    }

    /// Whether the processor has `feature` (an LLVM or a kernel name).
    pub fn has(&self, feature: &str) -> bool {
        self.flags.contains(&normalize(feature))
    }

    /// The features of `required` the processor lacks, in order.
    pub fn missing<'a>(&self, required: &[&'a str]) -> Vec<&'a str> {
        required.iter().copied().filter(|f| !self.has(f)).collect()
    }

    /// The processor's AVX-512 flags (normalized), for a report.
    pub fn avx512_flags(&self) -> Vec<&str> {
        self.flags.iter().map(String::as_str).filter(|f| f.starts_with("avx512")).collect()
    }
}

/// What a test does with a program needing AVX-512.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Execution {
    /// Run it: the command prefix (empty: natively), and how, for the
    /// report (`native: <model>` or `runner: <command>`).
    Run { prefix: Vec<String>, how: String },
    /// The flags of `HOD_AVX512_CPUINFO` would let it run natively.
    DryRun(String),
    /// Not run, and why.
    Skip(String),
}

/// Where the processor's flags come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Host {
    /// x86-64 Linux, flags read from `/proc/cpuinfo`.
    X86(CpuInfo),
    /// Flags read from `HOD_AVX512_CPUINFO` (any host): never executed.
    Faked(CpuInfo),
    /// No flags to go by, and why (another architecture, an unreadable
    /// or malformed file).
    None(String),
}

impl Host {
    /// The host the tests run on: `HOD_AVX512_CPUINFO` when it is set,
    /// else `/proc/cpuinfo` on x86-64 Linux.
    pub fn detect() -> Host {
        if let Some(path) = std::env::var_os("HOD_AVX512_CPUINFO") {
            let shown = path.to_string_lossy().into_owned();
            return match std::fs::read_to_string(&path).ok().as_deref().and_then(CpuInfo::parse) {
                Some(cpu) => Host::Faked(cpu),
                None => Host::None(format!("no processor flags in {shown}")),
            };
        }
        if !(cfg!(target_arch = "x86_64") && cfg!(target_os = "linux")) {
            return Host::None(format!(
                "the host is {} {}, and HOD_X64_AVX512_RUNNER is unset",
                std::env::consts::ARCH,
                std::env::consts::OS
            ));
        }
        match std::fs::read_to_string("/proc/cpuinfo").ok().as_deref().and_then(CpuInfo::parse) {
            Some(cpu) => Host::X86(cpu),
            None => Host::None("no processor flags in /proc/cpuinfo".into()),
        }
    }
}

/// What to do with a program requiring the features `required` (LLVM
/// names), given the runner variable's value and the host.
pub fn decide(runner: Option<&str>, host: &Host, required: &[&str]) -> Execution {
    if let Some(runner) = runner {
        let words: Vec<String> = runner.split_whitespace().map(str::to_owned).collect();
        match words.first().map(String::as_str) {
            None => {}
            Some("native") => {
                let how = "runner: native (HOD_X64_AVX512_RUNNER)".to_owned();
                return Execution::Run { prefix: Vec::new(), how };
            }
            Some(_) => return Execution::Run { how: format!("runner: {runner}"), prefix: words },
        }
    }
    match host {
        Host::None(why) => Execution::Skip(why.clone()),
        Host::X86(cpu) | Host::Faked(cpu) => {
            let missing = cpu.missing(required);
            if !missing.is_empty() {
                return Execution::Skip(format!("missing {} on {}", missing.join(","), cpu.model));
            }
            match host {
                Host::Faked(_) => Execution::DryRun(format!(
                    "would execute natively on {} (flags from HOD_AVX512_CPUINFO)",
                    cpu.model
                )),
                _ => Execution::Run { prefix: Vec::new(), how: format!("native: {}", cpu.model) },
            }
        }
    }
}

/// [`decide`] for this process: `HOD_X64_AVX512_RUNNER` and
/// [`Host::detect`].
pub fn execution(required: &[&str]) -> Execution {
    let runner = std::env::var("HOD_X64_AVX512_RUNNER").ok();
    decide(runner.as_deref(), &Host::detect(), required)
}

/// Appends `program\toutcome\tdetail` to the file `HOD_AVX512_REPORT`
/// names, when it is set (tabs and line breaks in the fields become
/// spaces). One write per line, in append mode, so the tests of one
/// binary running in parallel do not mix their lines.
pub fn report(program: &str, outcome: &str, detail: &str) {
    let Some(path) = std::env::var_os("HOD_AVX512_REPORT") else { return };
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let line = format!("{}\t{}\t{}\n", clean(program), clean(outcome), clean(detail));
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|e| panic!("HOD_AVX512_REPORT {}: {e}", path.to_string_lossy()));
    file.write_all(line.as_bytes()).expect("HOD_AVX512_REPORT: write");
}

/// Whether a program that is not executed is a failure
/// (`HOD_REQUIRE_AVX512=1`).
pub fn required() -> bool {
    std::env::var_os("HOD_REQUIRE_AVX512").is_some_and(|v| v == "1")
}

/// A program not executed: reports it (`outcome` is `skipped` or
/// `dry-run`) and says so on stderr; a failure under
/// `HOD_REQUIRE_AVX512=1`.
pub fn skip(program: &str, outcome: &str, why: &str) {
    report(program, outcome, why);
    eprintln!("{program}: not executed ({outcome}: {why})");
    assert!(!required(), "{program}: not executed under HOD_REQUIRE_AVX512=1 ({outcome}: {why})");
}

#[cfg(test)]
mod tests {
    use super::{CpuInfo, Execution, Host, decide, normalize};

    /// `/proc/cpuinfo` of an Intel Xeon with AVX-512 (Ice Lake server, as
    /// a cloud virtual machine lists it), first processor and the start
    /// of the second.
    const XEON: &str = "processor\t: 0\n\
        vendor_id\t: GenuineIntel\n\
        cpu family\t: 6\n\
        model\t\t: 106\n\
        model name\t: Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz\n\
        flags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 \
        clflush mmx fxsr sse sse2 ss ht syscall nx pdpe1gb rdtscp lm constant_tsc rep_good nopl \
        xtopology cpuid pni pclmulqdq ssse3 fma cx16 pcid sse4_1 sse4_2 movbe popcnt aes xsave \
        avx f16c rdrand hypervisor lahf_lm abm 3dnowprefetch invpcid_single fsgsbase bmi1 hle \
        avx2 smep bmi2 erms invpcid rtm avx512f avx512dq rdseed adx smap avx512ifma clflushopt \
        clwb avx512cd sha_ni avx512bw avx512vl xsaveopt xsavec xgetbv1 xsaves avx512vbmi umip \
        avx512_vbmi2 gfni vaes vpclmulqdq avx512_vnni avx512_bitalg avx512_vpopcntdq la57 rdpid \
        fsrm arch_capabilities\n\
        bugs\t\t: spectre_v1 spectre_v2\n\
        \n\
        processor\t: 1\n\
        model name\t: another\n\
        flags\t\t: fpu\n";

    /// `/proc/cpuinfo` of an AMD EPYC 7763 (Zen 3: AVX2, no AVX-512).
    const EPYC: &str = "processor\t: 0\n\
        vendor_id\t: AuthenticAMD\n\
        model name\t: AMD EPYC 7763 64-Core Processor\n\
        flags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 \
        clflush mmx fxsr sse sse2 ht syscall nx mmxext fxsr_opt pdpe1gb rdtscp lm constant_tsc \
        rep_good nopl tsc_reliable nonstop_tsc cpuid extd_apicid aperfmperf pni pclmulqdq ssse3 \
        fma cx16 pcid sse4_1 sse4_2 movbe popcnt aes xsave avx f16c rdrand hypervisor lahf_lm \
        cmp_legacy svm cr8_legacy abm sse4a misalignsse 3dnowprefetch osvw topoext \
        invpcid_single vmmcall fsgsbase bmi1 avx2 smep bmi2 erms invpcid rdseed adx smap \
        clflushopt clwb sha_ni xsaveopt xsavec xgetbv1 xsaves clzero xsaveerptr rdpru arat npt \
        nrip_save tsc_scale vmcb_clean flushbyasid decodeassists pausefilter pfthreshold \
        v_vmsave_vmload umip vaes vpclmulqdq rdpid fsrm\n";

    /// An AArch64 kernel's: `Features`, no `flags`.
    const ARM: &str = "processor\t: 0\nBogoMIPS\t: 48.00\n\
        Features\t: fp asimd evtstrm aes pmull sha1 sha2 crc32 atomics\n\
        CPU implementer\t: 0x61\n";

    #[test]
    fn the_flags_of_the_first_processor() {
        let xeon = CpuInfo::parse(XEON).unwrap();
        assert_eq!(xeon.model, "Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz");
        assert!(xeon.has("avx512f") && xeon.has("avx512vl") && xeon.has("avx512bw"));
        // LLVM's names against the kernel's.
        assert!(xeon.has("avx512vbmi2") && xeon.has("avx512_vbmi2") && xeon.has("bmi"));
        assert!(xeon.has("avx512vpopcntdq") && xeon.has("gfni") && xeon.has("sse4.1"));
        assert_eq!(
            xeon.missing(&["avx512f", "avx512fp16", "avx512bf16"]),
            ["avx512fp16", "avx512bf16"]
        );
        assert_eq!(
            xeon.avx512_flags(),
            [
                "avx512bitalg",
                "avx512bw",
                "avx512cd",
                "avx512dq",
                "avx512f",
                "avx512ifma",
                "avx512vbmi",
                "avx512vbmi2",
                "avx512vl",
                "avx512vnni",
                "avx512vpopcntdq"
            ]
        );
        // The second processor's lines are not read.
        assert!(!xeon.has("another"));
        let epyc = CpuInfo::parse(EPYC).unwrap();
        assert_eq!(epyc.model, "AMD EPYC 7763 64-Core Processor");
        assert!(epyc.has("avx2") && epyc.has("vaes") && !epyc.has("avx512f"));
        assert!(epyc.avx512_flags().is_empty());
    }

    #[test]
    fn malformed_files_have_no_flags() {
        for text in [ARM, "", "garbage without colons\n", "flags\t:\nmodel name\t: x\n", "flags"] {
            assert_eq!(CpuInfo::parse(text), None, "{text:?}");
        }
        // Flags without a model name still count.
        assert_eq!(CpuInfo::parse("flags : avx512f\n").unwrap().model, "unknown");
        assert_eq!(normalize("AVX512_FP16"), "avx512fp16");
    }

    #[test]
    fn the_runner_wins_then_the_flags_decide() {
        let xeon = Host::X86(CpuInfo::parse(XEON).unwrap());
        let epyc = Host::X86(CpuInfo::parse(EPYC).unwrap());
        let f = ["avx512f"];
        let native = |how: &str| Execution::Run { prefix: Vec::new(), how: how.into() };
        assert_eq!(
            decide(None, &xeon, &f),
            native("native: Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz")
        );
        assert_eq!(
            decide(None, &xeon, &["avx512f", "avx512fp16"]),
            Execution::Skip(
                "missing avx512fp16 on Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz".into()
            )
        );
        assert_eq!(
            decide(None, &epyc, &["avx512f", "avx512vl"]),
            Execution::Skip("missing avx512f,avx512vl on AMD EPYC 7763 64-Core Processor".into())
        );
        // An explicit runner wins, whatever the flags; an empty one does
        // not count.
        assert_eq!(
            decide(Some("native"), &epyc, &f),
            native("runner: native (HOD_X64_AVX512_RUNNER)")
        );
        assert_eq!(
            decide(Some("sde64 -spr --"), &epyc, &f),
            Execution::Run {
                prefix: vec!["sde64".into(), "-spr".into(), "--".into()],
                how: "runner: sde64 -spr --".into()
            }
        );
        assert_eq!(decide(Some("  "), &epyc, &f), decide(None, &epyc, &f));
        // No flags: skipped, as before detection.
        let none = Host::None("the host is aarch64 linux".into());
        assert_eq!(decide(None, &none, &f), Execution::Skip("the host is aarch64 linux".into()));
        // Faked flags never execute.
        let faked = Host::Faked(CpuInfo::parse(XEON).unwrap());
        assert!(matches!(decide(None, &faked, &f), Execution::DryRun(_)));
        assert!(matches!(decide(None, &faked, &["avx512fp16"]), Execution::Skip(_)));
    }
}
