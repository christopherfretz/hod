//! The target features of real `-march=` settings (WP-201,
//! `spec/codegen.md` §5.6.2 and §5.6.9, `spec/abi-x64.md` §1.3).
//!
//! - The census (`tests/features/x64_census.tsv`): what clang 19, clang 22
//!   and kiln write for every x86-64 CPU they accept. Every item of every
//!   string is accepted or refused by a named reason, never
//!   `attr:target-feature-unknown`.
//! - The judgement (`tests/features/x64_features.tsv`): every feature of
//!   the census and of `llc` 19's and 22's lists, `+` and `-`, with its
//!   outcome; and the CPUs against their outcomes
//!   (`tests/features/x64_cpus.tsv`). `HOD_BLESS=1` rewrites both tables
//!   from the code; a change shows in the diff.
//! - The evidence: `llc`'s calling convention (where each argument and
//!   result of a set of signatures travels, which registers a call keeps)
//!   under every census string, every feature enabled alone and every
//!   feature disabled on the richest CPUs, against the convention without
//!   features. Wherever `llc` differs, hod refuses (the feature, or the
//!   signature under the function's AVX and AVX-512).
//! - Execution: hod's functions built with the `haswell`, `skylake` and
//!   `znver4` strings call `llc`'s and are called by them, under the
//!   emulator with every CPU feature it models.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use aggr_gen::{PIE_FLAGS, header, llc, llc_of, parse, pic};
use hod_cg::{CgError, Stress};
use hod_testutil::{Scratch, run_with};
use hod_x64::features::{self, UNKNOWN};
use linking::Toolchain;

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/features");

/// One line of the census.
struct Census {
    tools: String,
    flag: String,
    cpu: Option<String>,
    features: Option<String>,
}

fn census() -> Vec<Census> {
    let text = std::fs::read_to_string(format!("{DATA}/x64_census.tsv")).unwrap();
    let absent = |s: &str| (s != "-").then(|| s.to_owned());
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            assert_eq!(f.len(), 5, "{line}");
            Census {
                tools: f[0].into(),
                flag: f[1].into(),
                cpu: absent(f[2]),
                features: absent(f[4]),
            }
        })
        .collect()
}

/// clang's `-march=native` for x86-64 on the AArch64 hosts in use: clang
/// writes the host's AArch64 features (`+neon`, `+lse`, ...) into an
/// x86-64 string. No x86-64 CPU has them; hod declines such a module
/// (`llc` ignores them with a warning). kiln writes the baseline.
fn cross_native(line: &Census) -> bool {
    line.flag == "-march=native" && line.features.as_deref().is_some_and(|f| f.contains("+neon"))
}

/// §5.6.2: every item of every census string is accepted or refused by
/// name; no item is unknown, on either LLVM version or kiln.
#[test]
fn census_strings_are_judged() {
    let lines = census();
    let mut accepted = 0;
    for line in &lines {
        let string = line.features.as_deref().unwrap_or("");
        if cross_native(line) {
            assert!(features::check_function(string).is_err(), "{} {}", line.tools, line.flag);
            continue;
        }
        for item in string.split(',').filter(|i| !i.is_empty()) {
            if let Err(refused) = features::check_one(item) {
                assert_ne!(refused.reason, UNKNOWN, "{item} of {} {}", line.tools, line.flag);
            }
        }
        assert_eq!(features::check_function(string), Ok(()), "{} {}", line.tools, line.flag);
        accepted += 1;
    }
    // The CPUs, the tools and the strings the census has.
    let cpus: BTreeSet<_> = lines.iter().filter_map(|l| l.cpu.as_deref()).collect();
    for want in ["haswell", "skylake", "znver4", "x86-64-v4", "diamondrapids", "nehalem"] {
        assert!(cpus.contains(want), "{want}");
    }
    for tool in ["clang19", "clang22", "kiln"] {
        assert!(lines.iter().any(|l| l.tools.split(',').any(|t| t == tool)), "{tool}");
    }
    eprintln!("x64 census: {accepted} of {} strings accepted, {} CPUs", lines.len(), cpus.len());
}

/// §5.6.2: the tuning features are accepted either way, whatever they
/// are, as `target-cpu` and `tune-cpu` values are.
#[test]
fn tuning_is_accepted() {
    for name in [
        "prefer-256-bit",
        "prefer-128-bit",
        "prefer-no-gather",
        "fast-gather",
        "fast-variable-perlane-shuffle",
        "slow-3ops-lea",
        "slow-unaligned-mem-32",
        "false-deps-popcnt",
        "tuning-fast-imm-vector-shift",
        "idivq-to-divl",
        "macrofusion",
        "ermsb",
        "vzeroupper",
    ] {
        for sign in ["+", "-"] {
            assert_eq!(features::check_one(&format!("{sign}{name}")), Ok(()), "{sign}{name}");
        }
        assert!(features::tuning(name), "{name}");
    }
    // `target-cpu` is any value, with the features it implies.
    for line in census() {
        let _ = features::of_function(line.cpu.as_deref(), line.features.as_deref());
    }
}

/// The outcome of one item.
fn outcome(item: &str) -> &'static str {
    match features::check_one(item) {
        Ok(()) if features::tuning(&item[1..]) => "tuning",
        Ok(()) => "accepted",
        Err(refused) => refused.reason,
    }
}

/// The names of `llc`'s x86 features (`-mattr=help`), if installed.
fn llc_features(major: u32) -> Option<Vec<String>> {
    llc_help(major, "Available features for this target:")
}

/// The names of `llc`'s x86-64 CPUs (`-mcpu=help`), if installed.
fn llc_cpus(major: u32) -> Option<Vec<String>> {
    llc_help(major, "Available CPUs for this target:")
}

fn llc_help(major: u32, heading: &str) -> Option<Vec<String>> {
    let llc = llc_of(major)?;
    let mut command = Command::new(llc);
    command.args(["-mtriple=x86_64-unknown-linux-gnu", "-mattr=help", "-o", "/dev/null"]);
    let outcome = run_with(&mut command, b"", Duration::from_mins(1));
    let text = outcome.stdout_text() + &outcome.stderr_text();
    let after = text.split_once(heading)?.1;
    let mut names = Vec::new();
    for line in after.lines().skip_while(|l| l.trim().is_empty()) {
        let Some(name) = line.split_whitespace().next() else { break };
        if !line.starts_with(char::is_whitespace) || name == "Use" {
            break;
        }
        names.push(name.to_owned());
    }
    Some(names)
}

/// Rewrites `file` with `text` under `HOD_BLESS=1`; else compares.
fn check_table(file: &str, text: &str) {
    let path = format!("{DATA}/{file}");
    if std::env::var_os("HOD_BLESS").is_some() {
        std::fs::write(&path, text).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap();
    for (n, (got, want)) in text.lines().zip(want.lines()).enumerate() {
        assert_eq!(got, want, "{file} line {}: run with HOD_BLESS=1 and review the diff", n + 1);
    }
    assert_eq!(text.lines().count(), want.lines().count(), "{file}: HOD_BLESS=1");
}

/// §5.6.2: the judgement table, feature by feature: every name the census
/// writes and every name `llc` 19 and 22 know, with the outcome of `+` and
/// `-` and how many census CPUs carry it. Every `llc` name is judged.
#[test]
fn every_feature_is_judged() {
    let mut carried: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in census().iter().filter(|l| !cross_native(l)) {
        for item in line.features.as_deref().unwrap_or("").split(',').filter(|i| !i.is_empty()) {
            let cpus = carried.entry(item[1..].to_owned()).or_default();
            cpus.insert(line.cpu.clone().unwrap_or_default());
        }
    }
    let mut names: BTreeSet<String> = carried.keys().cloned().collect();
    // The table is written from what the source knows, so that it does not
    // depend on which LLVM is installed; installed lists are checked
    // against it.
    let table = std::fs::read_to_string(format!("{DATA}/x64_features.tsv")).unwrap_or_default();
    names.extend(
        table
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split('\t').next())
            .map(str::to_owned),
    );
    for major in [19, 22] {
        let Some(list) = llc_features(major) else { continue };
        assert!(list.len() > 150, "llc {major}: {list:?}");
        for name in list {
            assert_ne!(outcome(&format!("+{name}")), UNKNOWN, "llc {major}: +{name}");
            assert_ne!(outcome(&format!("-{name}")), UNKNOWN, "llc {major}: -{name}");
            names.insert(name);
        }
    }
    let mut text = String::from(
        "# WP-201: every x86-64 feature of the census and of llc 19 and 22.\n\
         # name<TAB>outcome of +name<TAB>outcome of -name<TAB>census CPUs carrying it\n",
    );
    for name in &names {
        let (on, off) = (outcome(&format!("+{name}")), outcome(&format!("-{name}")));
        assert_ne!(on, UNKNOWN, "+{name}");
        assert_ne!(off, UNKNOWN, "-{name}");
        let count = carried.get(name).map_or(0, BTreeSet::len);
        writeln!(text, "{name}\t{on}\t{off}\t{count}").unwrap();
    }
    check_table("x64_features.tsv", &text);
}

/// §5.6.9: the CPUs against their outcomes: for each CPU of the census
/// and each tool, the string's outcome and the AVX and AVX-512 hod
/// assumes for a function with it.
#[test]
fn cpus_against_outcomes() {
    let mut rows: BTreeMap<(String, String), String> = BTreeMap::new();
    for line in census() {
        if line.flag == "(none)" || line.flag.ends_with("=native") {
            continue;
        }
        let string = line.features.as_deref().unwrap_or("");
        let verdict = features::check_function(string).map_or_else(|r| r.reason, |()| "accepted");
        let abi = features::of_function(line.cpu.as_deref(), line.features.as_deref()).abi;
        let cpu = line.flag.trim_start_matches("-march=").to_owned();
        let row = format!(
            "{verdict}\t{}\t{}",
            if abi.avx { "avx" } else { "-" },
            if abi.avx512 { "avx512" } else { "-" }
        );
        for tool in line.tools.split(',') {
            rows.insert((cpu.clone(), tool.to_owned()), row.clone());
        }
    }
    let mut text = String::from(
        "# WP-201: -march=<cpu> against the outcome of what each tool writes, and the\n\
         # AVX and AVX-512 hod assumes for its signatures (spec/abi-x64.md §1.3).\n\
         # cpu<TAB>tool<TAB>outcome<TAB>avx<TAB>avx512\n",
    );
    for ((cpu, tool), row) in &rows {
        assert!(row.starts_with("accepted"), "{cpu} {tool}: {row}");
        writeln!(text, "{cpu}\t{tool}\t{row}").unwrap();
    }
    check_table("x64_cpus.tsv", &text);
}

// --- llc's convention under each string -------------------------------------

/// The signatures (return, parameters, convention) whose convention is
/// compared. Each probe `p<k>_<s>` passes its arguments to `c<s>` and
/// returns its result; `p<k>_va` calls a variadic function.
const SIGS: &[(&str, &str, &str)] = &[
    ("i32", "i32, i64, ptr", "ccc"),
    ("double", "float, double", "ccc"),
    ("i128", "i128", "ccc"),
    ("x86_fp80", "x86_fp80", "ccc"),
    ("<4 x float>", "<4 x float>, <2 x double>", "ccc"),
    ("<8 x float>", "<8 x float>", "ccc"),
    ("<4 x double>", "<4 x double>", "ccc"),
    ("<16 x float>", "<16 x float>", "ccc"),
    ("<8 x i64>", "<8 x i64>", "ccc"),
    ("<32 x i16>", "<32 x i16>", "ccc"),
    ("<4 x i1>", "<4 x i1>", "ccc"),
    ("<8 x i1>", "<16 x i1>", "ccc"),
    ("<2 x i1>", "<64 x i1>", "ccc"),
    ("{float, float}", "{double, double}", "ccc"),
    ("half", "half, bfloat", "ccc"),
    ("<8 x half>", "<8 x half>, <8 x bfloat>", "ccc"),
    ("<16 x half>", "<16 x half>", "ccc"),
    ("<1 x i64>", "<1 x i64>, <2 x i32>", "ccc"),
    ("<2 x float>", "<3 x float>, <2 x float>", "ccc"),
    ("{<8 x float>}", "{<4 x double>}", "ccc"),
    (
        "void",
        "double, double, double, double, double, double, double, double, double, double, i64, \
         i64, i64, i64, i64, i64, i64, <4 x float>, <4 x float>",
        "ccc",
    ),
    ("{double, double, double}", "i8 zeroext, i16 signext", "fastcc"),
    ("i64", "i64", "preserve_mostcc"),
    ("i64", "i64", "preserve_allcc"),
    ("void", "ptr byval({i64, i64, i64}) align 8", "ccc"),
];

/// The probe of [`SIGS`] that is `preserve_allcc` (see
/// [`preserve_all_under_avx_is_llc_s`]).
const PRESERVE_ALL: usize = 23;

/// Splits a type list at its top-level commas.
fn types(list: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0, 0);
    for (i, c) in list.char_indices() {
        match c {
            '{' | '<' | '(' => depth += 1,
            '}' | '>' | ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(list[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(list[start..].trim());
    out
}

/// The probe definitions for attribute group `k`.
fn probes(k: usize, sigs: &[usize]) -> String {
    let mut out = String::new();
    for &s in sigs {
        let (ret, params, cc) = SIGS[s];
        let args: Vec<String> =
            types(params).iter().enumerate().map(|(i, t)| format!("{t} %a{i}")).collect();
        let args = args.join(", ");
        if ret == "void" {
            writeln!(
                out,
                "define {cc} void @p{k}_{s}({args}) #{k} {{\n  call {cc} void @c{s}({args})\n  \
                 ret void\n}}"
            )
        } else {
            writeln!(
                out,
                "define {cc} {ret} @p{k}_{s}({args}) #{k} {{\n  %r = call {cc} {ret} \
                 @c{s}({args})\n  ret {ret} %r\n}}"
            )
        }
        .unwrap();
    }
    out
}

fn declarations() -> String {
    let mut out = String::from("declare i32 @va(ptr, ...)\n");
    for (s, (ret, params, cc)) in SIGS.iter().enumerate() {
        writeln!(out, "declare {cc} {ret} @c{s}({params})").unwrap();
    }
    out
}

fn attributes(k: usize, cpu: &str, features: Option<&str>) -> String {
    let features = features.map_or(String::new(), |f| format!(" \"target-features\"=\"{f}\""));
    format!("attributes #{k} = {{ nounwind \"target-cpu\"=\"{cpu}\"{features} }}\n")
}

/// A module of the probes of every configuration.
fn probe_module(configs: &[(String, Option<String>)]) -> String {
    let all: Vec<usize> = (0..SIGS.len()).collect();
    let mut text = header() + &declarations();
    for (k, (cpu, features)) in configs.iter().enumerate() {
        text += &probes(k, &all);
        writeln!(
            text,
            "define i32 @p{k}_va() #{k} {{\n  %r = call i32 (ptr, ...) @va(ptr null, double 1.0, \
             i32 3, <4 x float> zeroinitializer, x86_fp80 0xK3FFF8000000000000000)\n  ret i32 %r\n}}"
        )
        .unwrap();
        text += &attributes(k, cpu, features.as_deref());
    }
    text
}

/// Splits an instruction's operands at the commas outside parentheses.
fn operands(line: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0, 0);
    for (i, c) in line.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(line[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(line[start..].trim());
    out
}

/// The convention of each probe in `llc`'s MIR after instruction
/// selection: where its arguments arrive (`liveins`, the fixed stack
/// objects), where its result leaves (the return's implicit uses), and of
/// its call: the registers it keeps (the register mask) and where the
/// result comes back (implicit defs; for the variadic call, the implicit
/// uses too, `al` among them). The stack pointer's and shadow stack's
/// operands are left out (they follow the code, not the convention).
fn conventions(mir: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for doc in mir.split("\n---\n") {
        let Some(name) = doc.lines().find_map(|l| l.strip_prefix("name:")).map(str::trim) else {
            continue;
        };
        if !name.starts_with('p') {
            continue;
        }
        let mut lines = Vec::new();
        let mut section = "";
        let mut fixed = String::new();
        let mut liveins = String::new();
        for line in doc.lines() {
            if !line.starts_with(' ') {
                section = line.split(':').next().unwrap_or("");
                continue;
            }
            match section {
                "liveins" => {
                    if let Some(reg) = line.split("reg: '").nth(1) {
                        liveins += reg.split('\'').next().unwrap_or("");
                        liveins += ",";
                    }
                }
                "fixedStack" => {
                    for key in ["offset: ", "size: "] {
                        if let Some(v) = line.split(key).nth(1) {
                            fixed += v.split(',').next().unwrap_or("");
                            fixed += " ";
                        }
                    }
                }
                _ => {}
            }
            let inst = line.trim();
            let call = inst.starts_with("CALL64") || inst.starts_with("TCRETURN");
            if !(call || inst.starts_with("RET")) {
                continue;
            }
            let variadic = inst.contains("@va,");
            if call && !variadic && !inst.split(['@', ',']).any(|w| w.starts_with('c')) {
                continue;
            }
            let mut kept = Vec::new();
            for op in operands(inst) {
                let words: Vec<&str> = op.split_whitespace().collect();
                if op.starts_with("RET") {
                    kept.push(words[0].trim_end_matches("64").to_owned());
                    continue;
                }
                if op.contains("csr") || op.contains("RegMask") {
                    kept.push(op.to_owned());
                    continue;
                }
                match words.as_slice() {
                    [_, "$rsp" | "$ssp"] => {}
                    ["implicit-def", reg] => kept.push(format!("def {reg}")),
                    ["implicit", reg] if variadic || !call => kept.push(format!("use {reg}")),
                    _ => {}
                }
            }
            lines.push(kept.join(" "));
        }
        lines.insert(0, format!("in {liveins} stack {fixed}"));
        out.insert(name.to_owned(), lines);
    }
    out
}

/// What hod does with probe `s` in a function whose features are
/// `abi`: compiles it (`true`) or refuses it.
fn hod_compiles(s: usize, abi: (bool, bool)) -> bool {
    let attrs = match abi {
        (false, false) => None,
        (true, false) => Some("+avx"),
        _ => Some("+avx512f"),
    };
    let text =
        header() + &declarations() + &probes(0, &[s]) + &attributes(0, "x86-64", attrs) + PIE_FLAGS;
    let (module, bodies) = parse(&text);
    match hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default())) {
        Ok(_) => true,
        Err(hod_x64::ModuleError::Function { error: CgError::Unsupported { .. }, .. }) => false,
        Err(other) => panic!("probe {s}: {other:?}"),
    }
}

/// The configurations of the evidence: every census string (and its CPU
/// alone), every feature of `names` enabled on `x86-64`, and every one
/// disabled on the richest CPUs and the plainest.
fn configurations(names: &BTreeSet<String>) -> Vec<(String, Option<String>)> {
    let mut configs: BTreeSet<(String, Option<String>)> = BTreeSet::new();
    for line in census().iter().filter(|l| !cross_native(l)) {
        let cpu = line.cpu.clone().unwrap_or_else(|| "x86-64".into());
        configs.insert((cpu.clone(), line.features.clone()));
        configs.insert((cpu, None));
    }
    for name in names {
        configs.insert(("x86-64".into(), Some(format!("+{name}"))));
        for cpu in ["x86-64-v4", "sapphirerapids", "diamondrapids", "znver5", "haswell", "x86-64"] {
            configs.insert((cpu.into(), Some(format!("-{name}"))));
        }
    }
    configs.into_iter().collect()
}

/// §5.6.2, §5.6.9, `spec/abi-x64.md` §1.3: for every configuration hod
/// accepts, every signature `llc` 19 or 22 passes, returns or calls
/// differently from a function without features is one hod refuses under
/// the function's AVX and AVX-512 (`x64:wide-vector-abi`), or one `llc`
/// passes exactly as under plain `+avx` (or `+avx512f`) on `x86-64`: the
/// convention hod's `ymm` assignment follows, which the wide grid checks
/// against `llc` (`abi::wide_vector_grid`). A configuration
/// `llc` cannot compile (a CPU it does not know, `-x87` with an
/// `x86_fp80`) or hod refuses by the feature needs no evidence.
#[test]
fn feature_conventions_agree_with_llc() {
    let scratch = Scratch::new("x64-wp201-conventions");
    let mut names: BTreeSet<String> = BTreeSet::new();
    for major in [19, 22] {
        names.extend(llc_features(major).unwrap_or_default());
    }
    let configs = configurations(&names);
    let mut verdicts: BTreeMap<(usize, (bool, bool)), bool> = BTreeMap::new();
    for major in [19, 22] {
        let (Some(cpus), Some(llc)) = (llc_cpus(major), llc_of(major)) else { continue };
        let usable: Vec<_> = configs
            .iter()
            .filter(|(cpu, f)| {
                cpus.contains(cpu) && {
                    let f = f.as_deref().unwrap_or("");
                    features::check_function(f).is_ok()
                        && !features::of_function(None, Some(f)).no_x87
                }
            })
            .cloned()
            .collect();
        let base = {
            let text = probe_module(&[("x86-64".into(), None)]);
            mir_conventions(&scratch, &llc, major, "base", &text)
        };
        // The conventions hod follows when it compiles a probe: `p0_*`
        // under AVX, `p1_*` under AVX-512.
        let reference = {
            let text = probe_module(&[
                ("x86-64".into(), Some("+avx".into())),
                ("x86-64".into(), Some("+avx512f".into())),
            ]);
            mir_conventions(&scratch, &llc, major, "reference", &text)
        };
        let (mut checked, mut differing, mut preserve_all, mut followed) = (0, 0, 0, 0);
        for (chunk_no, chunk) in usable.chunks(64).enumerate() {
            let text = probe_module(chunk);
            let got = mir_conventions(&scratch, &llc, major, &format!("c{chunk_no}"), &text);
            for (k, (cpu, f)) in chunk.iter().enumerate() {
                let abi = features::of_function(Some(cpu), f.as_deref()).abi;
                let abi = (abi.avx, abi.avx512);
                let mut sigs: Vec<String> = (0..SIGS.len()).map(|s| s.to_string()).collect();
                sigs.push("va".into());
                for s in &sigs {
                    let (theirs, base) = (&got[&format!("p{k}_{s}")], &base[&format!("p0_{s}")]);
                    if theirs == base {
                        continue;
                    }
                    differing += 1;
                    let n: usize = s.parse().unwrap_or(usize::MAX);
                    if n == PRESERVE_ALL {
                        // `preserve_allcc`'s mask keeps all of `ymm0`..`ymm15`
                        // under AVX, which `llc`'s own callee does not honour
                        // (`preserve_all_under_avx_is_llc_s`).
                        assert!(abi.0, "{cpu} {f:?}: preserve_allcc without AVX");
                        preserve_all += 1;
                        continue;
                    }
                    assert!(n != usize::MAX, "llc {major} {cpu} {f:?}: the variadic call");
                    let compiles =
                        *verdicts.entry((n, abi)).or_insert_with(|| hod_compiles(n, abi));
                    if !compiles {
                        continue;
                    }
                    assert!(
                        abi.0,
                        "llc {major} {cpu} {f:?}: {} differs and hod compiles it without AVX\n\
                         llc: {theirs:?}\nwithout features: {base:?}",
                        SIGS[n].1
                    );
                    let own = &reference[&format!("p{}_{s}", usize::from(abi.1))];
                    assert!(
                        theirs == own,
                        "llc {major} {cpu} {f:?}: {} differs and hod compiles it with {abi:?}\n\
                         llc: {theirs:?}\nunder the reference: {own:?}",
                        SIGS[n].1
                    );
                    followed += 1;
                }
                checked += 1;
            }
        }
        eprintln!(
            "x64 wp201: llc {major}: {checked} configurations, {differing} probes differ \
             ({followed} compiled by hod as under the reference, the rest refused), \
             {preserve_all} of them preserve_allcc under AVX"
        );
    }
}

fn mir_conventions(
    scratch: &Scratch,
    llc: &Path,
    major: u32,
    name: &str,
    text: &str,
) -> BTreeMap<String, Vec<String>> {
    let input = scratch.write(&format!("{name}-{major}.ll"), text);
    let mut command = Command::new(llc);
    command.args(["-O0", "-stop-after=finalize-isel", "-o", "-"]).arg(&input);
    let outcome = run_with(&mut command, b"", Duration::from_mins(5));
    assert!(outcome.success(), "llc {major} {name}: {}", outcome.stderr_text());
    conventions(&outcome.stdout_text())
}

// --- Execution ----------------------------------------------------------------

/// hod's side: each `@h_*` calls an `@l_*` and is called by a `@d_*`.
const MIX_HOD: &str = "
declare double @l_mix(double, float, <4 x float>, <2 x double>, i64)
declare <4 x float> @l_vec(<4 x float>, <4 x float>)
declare { double, double } @l_pair(double, double)
declare i128 @l_wide(i128, float)

define double @h_mix(double %a, float %b, <4 x float> %v, <2 x double> %w, i64 %n) #0 {
  %r = call double @l_mix(double %a, float %b, <4 x float> %v, <2 x double> %w, i64 %n)
  %s = fadd double %r, 0.5
  ret double %s
}
define <4 x float> @h_vec(<4 x float> %a, <4 x float> %b) #0 {
  %r = call <4 x float> @l_vec(<4 x float> %b, <4 x float> %a)
  %s = fsub <4 x float> %r, %a
  ret <4 x float> %s
}
define { double, double } @h_pair(double %a, double %b) #0 {
  %r = call { double, double } @l_pair(double %b, double %a)
  ret { double, double } %r
}
define i128 @h_wide(i128 %a, float %f) #0 {
  %r = call i128 @l_wide(i128 %a, float %f)
  %s = add i128 %r, 1
  ret i128 %s
}
";

/// `llc`'s side: the callees, and the drivers C calls through memory.
const MIX_LLC: &str = "
declare double @h_mix(double, float, <4 x float>, <2 x double>, i64)
declare <4 x float> @h_vec(<4 x float>, <4 x float>)
declare { double, double } @h_pair(double, double)
declare i128 @h_wide(i128, float)

define double @l_mix(double %a, float %b, <4 x float> %v, <2 x double> %w, i64 %n) #0 {
  %bd = fpext float %b to double
  %v0 = extractelement <4 x float> %v, i32 0
  %v3 = extractelement <4 x float> %v, i32 3
  %v03 = fmul float %v0, %v3
  %vd = fpext float %v03 to double
  %w1 = extractelement <2 x double> %w, i32 1
  %nd = sitofp i64 %n to double
  %s1 = fadd double %a, %bd
  %s2 = fadd double %s1, %vd
  %s3 = fsub double %s2, %w1
  %s4 = fmul double %s3, %nd
  ret double %s4
}
define <4 x float> @l_vec(<4 x float> %a, <4 x float> %b) #0 {
  %m = fmul <4 x float> %a, %b
  %r = fadd <4 x float> %m, <float 1.0, float 2.0, float 3.0, float 4.0>
  ret <4 x float> %r
}
define { double, double } @l_pair(double %a, double %b) #0 {
  %d = fsub double %a, %b
  %p = fmul double %a, %b
  %r0 = insertvalue { double, double } poison, double %d, 0
  %r1 = insertvalue { double, double } %r0, double %p, 1
  ret { double, double } %r1
}
define i128 @l_wide(i128 %a, float %f) #0 {
  %i = fptosi float %f to i128
  %r = mul i128 %a, %i
  ret i128 %r
}

define void @d_mix(ptr %in, ptr %out) #0 {
  %a = load double, ptr %in
  %pb = getelementptr i8, ptr %in, i64 8
  %b = load float, ptr %pb
  %pv = getelementptr i8, ptr %in, i64 16
  %v = load <4 x float>, ptr %pv
  %pw = getelementptr i8, ptr %in, i64 32
  %w = load <2 x double>, ptr %pw
  %r = call double @h_mix(double %a, float %b, <4 x float> %v, <2 x double> %w, i64 -3)
  store double %r, ptr %out
  %pa = getelementptr i8, ptr %out, i64 8
  %pr = call { double, double } @h_pair(double %a, double 0.25)
  store { double, double } %pr, ptr %pa
  %pq = getelementptr i8, ptr %out, i64 32
  %vr = call <4 x float> @h_vec(<4 x float> %v, <4 x float> <float 0.5, float -1.0, float 8.0, float 3.0>)
  store <4 x float> %vr, ptr %pq
  %wi = call i128 @h_wide(i128 -81985529216486895, float 3.0)
  %pi = getelementptr i8, ptr %out, i64 48
  store i128 %wi, ptr %pi
  ret void
}
";

const MIX_MAIN: &str = r#"
#include <stdio.h>
#include <string.h>
#include <stdint.h>
void d_mix(const void *in, void *out);
int main(void) {
    _Alignas(16) unsigned char in[48], out[64];
    double a = 1.75; float b = -2.5f;
    float v[4] = {3.0f, -1.5f, 0.25f, 2.0f}; double w[2] = {10.0, -4.125};
    memcpy(in, &a, 8); memcpy(in + 8, &b, 4); memcpy(in + 16, v, 16); memcpy(in + 32, w, 16);
    d_mix(in, out);
    double r[4]; float q[4]; uint64_t i[2];
    memcpy(r, out, 32); memcpy(q, out + 32, 16); memcpy(i, out + 48, 16);
    printf("%.17g %.17g %.17g\n", r[0], r[1], r[2]);
    printf("%.9g %.9g %.9g %.9g\n", q[0], q[1], q[2], q[3]);
    printf("%016llx%016llx\n", (unsigned long long)i[1], (unsigned long long)i[0]);
    return 0;
}
"#;

/// The census string clang 22 writes for `-march=<cpu>`.
fn clang22_string(cpu: &str) -> String {
    census()
        .into_iter()
        .find(|l| l.flag == format!("-march={cpu}") && l.tools.contains("clang22"))
        .and_then(|l| l.features)
        .unwrap_or_else(|| panic!("{cpu} in the census"))
}

/// Links `objects` with each linker and runs the program under the
/// emulator's richest CPU; the output is the same for every linker.
fn link_and_run(toolchain: &Toolchain, scratch: &Scratch, tag: &str, objects: &[&Path]) -> String {
    let mut seen: Option<String> = None;
    for linker in &toolchain.linkers {
        let exe = scratch.join(format!("{tag}-{}", linker.name));
        let ran = toolchain.link_and_run(linker, &["-pie"], objects, &exe);
        assert!(
            ran.success(),
            "{tag} ({}): {}{}",
            linker.name,
            ran.stdout_text(),
            ran.stderr_text()
        );
        let out = ran.stdout_text();
        if let Some(seen) = &seen {
            assert_eq!(&out, seen, "{tag}: {} differs", linker.name);
        }
        seen = Some(out);
    }
    seen.unwrap_or_default()
}

fn module(body: &str, attrs: &str) -> String {
    format!("{}{body}\nattributes #0 = {{ {attrs} }}\n{PIE_FLAGS}", header())
}

/// §5.6.2: hod's functions built with what clang 22 writes for
/// `-march=haswell`, `skylake` and `znver4` call `llc`'s and are called
/// by `llc`'s (built with the same string; for `znver4` without AVX-512,
/// which the emulator does not model and which changes none of these
/// signatures: `feature_conventions_agree_with_llc`). The program prints
/// what an all-`llc` build prints.
#[test]
fn march_strings_mix_with_llc() {
    let scratch = Scratch::new("x64-wp201-mix");
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let toolchain = toolchain.with_emulated_cpu("max");
    let main = toolchain.compile_c(&scratch, "mix-main", MIX_MAIN, &["-fPIE"]);
    for cpu in ["haswell", "skylake", "znver4"] {
        let string = clang22_string(cpu);
        let attrs = format!("nounwind \"target-cpu\"=\"{cpu}\" \"target-features\"=\"{string}\"");
        let runnable = if cpu == "znver4" {
            format!(
                "nounwind \"target-cpu\"=\"znver3\" \"target-features\"=\"{}\"",
                clang22_string("znver3")
            )
        } else {
            attrs.clone()
        };
        assert_eq!(features::check_function(&string), Ok(()), "{cpu}");
        let hod_side = module(MIX_HOD, &attrs);
        let hod_object = {
            let (m, bodies) = parse(&hod_side);
            let bytes = hod_x64::compile_to_object(&m, &bodies, &pic(Stress::default()))
                .unwrap_or_else(|e| panic!("hod refused {cpu}: {e:?}"));
            scratch.write(&format!("mix-hod-{cpu}.o"), bytes)
        };
        for major in [19, 22] {
            let Some(callees) = llc(
                &scratch,
                major,
                &format!("mix-llc-{cpu}-{major}"),
                &module(MIX_LLC, &runnable),
            ) else {
                continue;
            };
            let Some(llc_hod_side) =
                llc(&scratch, major, &format!("mix-h-{cpu}-{major}"), &module(MIX_HOD, &runnable))
            else {
                continue;
            };
            let expected = link_and_run(
                &toolchain,
                &scratch,
                &format!("mix-llc-{cpu}-{major}"),
                &[&main, &callees, &llc_hod_side],
            );
            let got = link_and_run(
                &toolchain,
                &scratch,
                &format!("mix-{cpu}-{major}"),
                &[&main, &callees, &hod_object],
            );
            assert_eq!(got, expected, "hod with -march={cpu} against llc {major}");
            assert_eq!(expected.lines().count(), 3, "{expected}");
        }
    }
}

/// `llc`'s side of the `preserve_allcc` program: the caller keeps an
/// `<8 x float>` in `ymm0` across the call (its register mask says all of
/// `ymm0`..`ymm15` survive under AVX), and the callee's callee writes
/// 256-bit registers.
const PA_LLC: &str = "
declare preserve_allcc void @h_pa(ptr)
@g = global <8 x float> zeroinitializer, align 32
define void @l_clobber(ptr %p) #0 {
  %v = load <8 x float>, ptr %p
  %w = fadd <8 x float> %v, %v
  store <8 x float> %w, ptr @g
  ret void
}
define void @d_keep(ptr %in, ptr %out) #0 {
  %v = load <8 x float>, ptr %in
  %x = fadd <8 x float> %v, <float 1.0, float 1.0, float 1.0, float 1.0, float 1.0, float 1.0, float 1.0, float 1.0>
  call preserve_allcc void @h_pa(ptr %in)
  store <8 x float> %x, ptr %out
  ret void
}
";

const PA_HOD: &str = "
declare void @l_clobber(ptr)
define preserve_allcc void @h_pa(ptr %p) #0 {
  call void @l_clobber(ptr %p)
  ret void
}
";

const PA_MAIN: &str = r#"
#include <stdio.h>
void d_keep(const float *in, float *out);
int main(void) {
    _Alignas(32) float in[8] = {1, 2, 3, 4, 5, 6, 7, 8}, out[8];
    d_keep(in, out);
    for (int i = 0; i < 8; i++) printf("%g ", out[i]);
    printf("\n");
    return 0;
}
"#;

/// §5.6.9: under AVX (any feature that implies it, or a CPU that has it)
/// `llc` 19 and 22 give a `preserve_allcc` call a register mask that keeps
/// all of `ymm0`..`ymm15`, and hod's `preserve_allcc` function saves only
/// their low halves (`spec/codegen.md` §5.6.23). But `llc`'s own
/// `preserve_allcc` function, after saving and restoring the whole `ymm`
/// registers, returns through `vzeroupper` (which clears every upper
/// half) whenever it called anything, and keeps nothing it does not
/// touch when it is a leaf, as hod's does: the caller's upper halves are
/// lost either way, and a program cannot tell hod's callee from `llc`'s.
/// Here a caller keeps an `<8 x float>` in `ymm0` across the call; both
/// callees give it back with its upper half zero.
#[test]
fn preserve_all_under_avx_is_llc_s() {
    let scratch = Scratch::new("x64-wp201-preserve-all");
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let toolchain = toolchain.with_emulated_cpu("max");
    let main = toolchain.compile_c(&scratch, "pa-main", PA_MAIN, &["-fPIE"]);
    let attrs = "nounwind \"target-cpu\"=\"haswell\"";
    let hod_text = module(PA_HOD, attrs);
    let (m, bodies) = parse(&hod_text);
    let object = scratch.write(
        "pa-hod.o",
        hod_x64::compile_to_object(&m, &bodies, &pic(Stress::default())).expect("hod compiles"),
    );
    for major in [19, 22] {
        let Some(llc_side) =
            llc(&scratch, major, &format!("pa-llc{major}"), &module(PA_LLC, attrs))
        else {
            continue;
        };
        // `llc` alone: its callee returns through `vzeroupper`.
        let Some(llc_pa) =
            llc(&scratch, major, &format!("pa-h-llc{major}"), &module(PA_HOD, attrs))
        else {
            continue;
        };
        let out = link_and_run(
            &toolchain,
            &scratch,
            &format!("pa-llc{major}"),
            &[&main, &llc_side, &llc_pa],
        );
        assert_eq!(out, "2 3 4 5 0 0 0 0 \n", "llc {major} alone");
        let out =
            link_and_run(&toolchain, &scratch, &format!("pa{major}"), &[&main, &llc_side, &object]);
        assert_eq!(out, "2 3 4 5 0 0 0 0 \n", "hod against llc {major}");
    }
}
