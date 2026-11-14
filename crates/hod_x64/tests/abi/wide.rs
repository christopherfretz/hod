//! WP-203: vectors wider than 128 bits under AVX (`spec/abi-x64.md` §1.3,
//! §3.2, §4.3, §5.1, §9.1): the grid of §11 for functions compiled for
//! the `-march` strings of WP-201's census and for `+avx` and `+avx2`
//! alone, hod's assignment under each against `llc` 19 and 22 in both
//! directions, run under `qemu-x86_64 -cpu max`.

use std::collections::{BTreeMap, HashSet};

use hod_testutil::{Scratch, run};
use hod_x64::abi::Features;

use crate::grid::{pressures, random_case, run_cases, scalars, seq, vectors};
use crate::ir;
use crate::run::{Machine, Tools};
use crate::sigs::{Attr, Blobs, Case, Plan, byval, p, pa, table};
use crate::ty::{I1, I8, I16, I32, I64, Rng, Ty, arr, st, vec};

/// The census (`tests/features/x64_census.tsv`).
const CENSUS: &str = include_str!("../features/x64_census.tsv");

/// The `target-cpu` and `target-features` clang 22 (else 19) writes for
/// `-march=<cpu>`.
fn census(cpu: &str) -> Machine {
    let flag = format!("-march={cpu}");
    let mut found = None;
    for line in CENSUS.lines().filter(|line| !line.starts_with('#')) {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() == 5 && fields[1] == flag {
            found = Some(Machine { cpu: fields[2].to_string(), features: fields[4].to_string() });
            if fields[0].contains("clang22") {
                break;
            }
        }
    }
    found.unwrap_or_else(|| panic!("{flag} is not in the census"))
}

/// The machines of the grid: `+avx` and `+avx2` on the baseline CPU, and
/// four `-march` strings of the census (AVX alone, AVX2, AVX-512, AMD).
pub fn machines() -> Vec<Machine> {
    let plain = |features: &str| Machine { cpu: "x86-64".into(), features: features.into() };
    vec![
        plain("+avx"),
        plain("+avx2"),
        census("sandybridge"),
        census("haswell"),
        census("skylake-avx512"),
        census("znver3"),
    ]
}

/// Vectors of every element type whose shape AVX changes or keeps
/// (`spec/abi-x64.md` §4.3): 256 bits, two and four times that, those
/// widened to 256 bits in one step, those LLVM passes element by element
/// with AVX too, and 128-bit neighbours.
pub fn wide_vectors() -> Vec<Ty> {
    let (h, b, f, d) = (Ty::Half, Ty::BFloat, Ty::Float, Ty::Double);
    let shapes: [(Ty, &[u32]); 10] = [
        (I1, &[8, 16, 32, 64]),
        (I8, &[17, 24, 32, 48, 64, 128]),
        (I16, &[9, 12, 16, 32, 64]),
        (I32, &[5, 6, 7, 8, 12, 16, 32]),
        (I64, &[3, 4, 5, 8, 16]),
        (Ty::Ptr, &[3, 4, 8]),
        (h, &[9, 12, 16, 32]),
        (b, &[9, 16, 32]),
        (f, &[5, 6, 7, 8, 9, 12, 16, 32]),
        (d, &[3, 4, 5, 8, 16]),
    ];
    let mut all = Vec::new();
    for (elem, lens) in shapes {
        all.extend(lens.iter().map(|&len| vec(elem.clone(), len)));
    }
    all
}

/// The aggregates clang and kiln pass `__m256` in, and others.
fn wide_aggregates() -> Vec<Ty> {
    let (v4i64, v8f32, v4f32) = (vec(I64, 4), vec(Ty::Float, 8), vec(Ty::Float, 4));
    let v4f64 = vec(Ty::Double, 4);
    vec![
        st(std::slice::from_ref(&v4i64)),
        st(&[v4i64.clone(), v4i64.clone()]),
        arr(v8f32.clone(), 2),
        st(&[arr(v8f32.clone(), 2)]),
        st(&[v4f32.clone(), v8f32.clone()]),
        st(&[v8f32.clone(), I32]),
        st(&[I8, v4f64.clone(), Ty::Double]),
        st(&[v4f64.clone(), v4f64.clone()]),
        st(&[v8f32.clone(), vec(Ty::Float, 16)]),
        Ty::Packed(vec![I8, v8f32.clone()]),
        arr(v4i64, 5),
    ]
}

/// The cases: every wide type under every pressure (as argument, return
/// and variadic), the aggregates, `byval` and `sret` around them, nine to
/// eleven `ymm` arguments, returns that share `xmm0`..`xmm3`.
pub fn wide_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut types = wide_vectors();
    types.extend(wide_aggregates());
    for ty in &types {
        cases.extend(pressures(ty));
        cases.push(Case::of(&seq(&[(&vec(Ty::Float, 4), 1), (ty, 2), (&I64, 1)])).ret(ty.clone()));
    }
    let (v4i64, v8f32, v4f32) = (vec(I64, 4), vec(Ty::Float, 8), vec(Ty::Float, 4));
    let d = Ty::Double;
    // Eight, nine and eleven `ymm` arguments, mixed with `xmm` ones.
    cases.push(Case::of(&seq(&[(&v4i64, 8), (&I32, 1), (&v4i64, 1)])).ret(v4i64.clone()));
    cases.push(Case::of(&seq(&[(&v8f32, 9), (&I32, 1), (&v4i64, 2), (&d, 1)])));
    cases.push(Case::of(&seq(&[(&v4f32, 3), (&v8f32, 5), (&d, 2), (&v8f32, 1)])));
    cases.push(Case::of(&seq(&[(&d, 7), (&v8f32, 2), (&v4f32, 1), (&I64, 7)])));
    // `byval` and `sret` holding wide vectors, as clang passes a
    // structure of one in memory.
    for pointee in [v4i64.clone(), st(&[v4i64.clone(), v4i64.clone()]), arr(v8f32.clone(), 2)] {
        cases.push(Case::new(vec![byval(pointee.clone(), Some(32), None), p(v8f32.clone())]));
        cases.push(Case::new(vec![
            pa(Ty::Ptr, Attr::Sret),
            p(v4i64.clone()),
            byval(pointee.clone(), Some(32), None),
            p(I32),
        ]));
        cases.push(Case::new(vec![p(I32), byval(pointee, Some(32), None), p(I8)]).variadic(1));
    }
    // Returns that share `xmm0`..`xmm3` with 128-bit pieces.
    for ret in [
        st(&[v4f32.clone(), v8f32.clone()]),
        st(&[v8f32.clone(), d.clone()]),
        st(&[v8f32.clone(), v4f32.clone(), v8f32.clone(), Ty::Float]),
        st(&[v8f32.clone(), v8f32.clone(), v8f32.clone(), v8f32.clone()]),
        st(&[v8f32.clone(), v8f32.clone(), v8f32.clone(), v8f32.clone(), v8f32.clone()]),
        vec(Ty::Float, 64),
    ] {
        cases.push(Case::of(&[v8f32.clone(), I16]).ret(ret));
    }
    // Variadic: named and unnamed `ymm` pieces.
    cases
        .push(Case::of(&[v8f32.clone(), I32, v8f32.clone(), d.clone(), v4f32.clone()]).variadic(1));
    cases.push(Case::of(&[I32, v8f32.clone(), v4i64.clone(), I16]).variadic(3).ret(v8f32));
    cases
}

/// The cases `assign` accepts under `features`, and the names of those
/// it refuses.
fn accepted_with(
    cases: Vec<Case>,
    features: Features,
) -> (Vec<Case>, BTreeMap<&'static str, usize>) {
    let mut types = table();
    let mut refused = BTreeMap::new();
    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    for case in cases {
        match case.assign_with(&mut types, features) {
            Ok(_) => {
                if seen.insert(case.clone()) {
                    kept.push(case);
                }
            }
            Err(name) => *refused.entry(name).or_insert(0) += 1,
        }
    }
    (kept, refused)
}

/// `count` random signatures over every scalar, the 128-bit grid's
/// vectors and the wide ones.
fn random_wide(seed: u64, count: usize) -> Vec<Case> {
    let mut rng = Rng(seed);
    let mut pool = scalars();
    pool.extend(vectors());
    pool.extend(wide_vectors());
    pool.extend(wide_vectors());
    (0..count).map(|_| random_case(&mut rng, &pool)).collect()
}

/// `spec/abi-x64.md` §1.3, §3.2, §4.3, §5.1, §9.1 (WP-203): the wide grid
/// on every machine of [`machines`], against the six `llc`
/// configurations, both directions. The names refused are those §1.3
/// keeps: `x64:wide-vector-abi` (beyond 256 bits, or a vector of `i1`,
/// with AVX-512), `x64:mask-vector`, and the conventions §1.2 refuses.
#[test]
fn wide_vector_grid() {
    let Some(base) = Tools::find() else { return };
    for machine in machines() {
        let label = format!("abi-wide-{}{}", machine.cpu, machine.features.len());
        let features = machine.abi();
        assert!(features.avx, "{machine:?} has AVX");
        let Some(tools) = base.with_machine(machine.clone()) else { return };
        let (mut cases, refused) = accepted_with(wide_cases(), features);
        // The random signatures `assign` accepts; what it refuses of them
        // (invalid ones among them) is only counted.
        let (random, random_refused) = accepted_with(random_wide(0x203, 300), features);
        cases.extend(random.into_iter().filter(|case| !cases.contains(case)).collect::<Vec<_>>());
        eprintln!("random signatures refused while drawing: {random_refused:?}");
        for name in refused.keys() {
            assert!(
                ["x64:wide-vector-abi", "x64:mask-vector", "abi:cc", "x64:nest"].contains(name),
                "{} refused as {name}",
                machine.cpu
            );
        }
        if features.avx512 {
            // `qemu`'s TCG has no AVX-512: code compiled for this machine
            // stops on its first EVEX instruction. The evidence is that
            // hod and `llc` place every accepted signature as under AVX
            // alone, which the other machines run (§11).
            let checked = same_as_avx(&label, &cases, &machine, &tools);
            eprintln!(
                "wide grid, {}: {} signatures, {checked} llc conventions equal to +avx; \
                 refused: {refused:?}",
                machine.cpu,
                cases.len()
            );
            continue;
        }
        let report = run_cases(&label, &cases, 0x203, &tools);
        eprintln!(
            "wide grid, {} {}: {} signatures; {}; refused: {refused:?}; llc crashed on: {:#?}",
            machine.cpu,
            &machine.features[..machine.features.len().min(24)],
            cases.len(),
            report.summary(),
            report.llc_crashes
        );
        report.check(&format!("wide grid {}", machine.cpu));
    }
}

/// `spec/abi-x64.md` §11, for a machine with AVX-512: hod assigns each of
/// `cases` as under AVX alone, and every `llc` at `-O0` (`FastISel` and
/// `SelectionDAG`) passes, receives and returns each the same way for
/// `machine` as for `x86-64` with `+avx`: the same live-in registers,
/// incoming stack objects and register operands of each call and return.
/// Returns how many function conventions were compared.
fn same_as_avx(label: &str, cases: &[Case], machine: &Machine, tools: &Tools) -> usize {
    let avx = Features { avx: true, avx512: false, assumed: false };
    let mut types = table();
    for case in cases {
        assert_eq!(
            case.assign_with(&mut types, machine.abi()),
            case.assign_with(&mut types, avx),
            "{}: `{}`",
            machine.cpu,
            case.text()
        );
    }
    let (mut blobs, mut rng) = (Blobs::default(), Rng(0x203));
    let plans: Vec<Plan> = cases
        .iter()
        .map(|case| Plan::new_with(case, &mut types, &mut blobs, &mut rng, avx))
        .collect();
    // LLVM has no `va_arg` for this target: no variadic LLVM callee.
    let runs = |index: usize, hod_calls: bool| !(hod_calls && plans[index].case.fixed.is_some());
    let reference = Machine { cpu: "x86-64".into(), features: "+avx".into() };
    let scratch = Scratch::new(label);
    let mut compared = 0;
    for (config, llc) in tools.llc.iter().filter(|(config, _)| config.opt == "-O0") {
        let mut got = Vec::new();
        for (name, attrs) in [("machine", machine.attributes()), ("avx", reference.attributes())] {
            let source =
                scratch.write(&format!("{name}.ll"), ir::module_with(&plans, &runs, Some(&attrs)));
            let outcome = run(
                llc,
                [
                    "-O0".as_ref(),
                    if config.dag { "-fast-isel=0" } else { "-fast-isel=1" }.as_ref(),
                    "-relocation-model=pic".as_ref(),
                    "-stop-after=finalize-isel".as_ref(),
                    "-o".as_ref(),
                    "-".as_ref(),
                    source.as_os_str(),
                ],
            );
            assert!(outcome.success(), "llc {} {name}: {}", config.tag(), outcome.stderr_text());
            got.push(conventions(&outcome.stdout_text()));
        }
        assert_eq!(got[0].len(), got[1].len(), "llc {}: the same functions", config.tag());
        for (function, theirs) in &got[0] {
            assert_eq!(
                Some(theirs),
                got[1].get(function),
                "llc {} {}: `{function}` differs from +avx",
                config.tag(),
                machine.cpu
            );
            compared += 1;
        }
    }
    compared
}

/// Each function's convention in `llc`'s MIR after instruction selection:
/// its live-in registers, its fixed stack objects (offset and size), and
/// the register operands of each call and return, without the stack
/// pointer's and the shadow stack's.
fn conventions(mir: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for doc in mir.split("\n---\n") {
        let Some(name) = doc.lines().find_map(|l| l.strip_prefix("name:")).map(str::trim) else {
            continue;
        };
        let mut lines = Vec::new();
        let mut section = "";
        for line in doc.lines() {
            if !line.starts_with(' ') {
                section = line.split(':').next().unwrap_or("");
                continue;
            }
            match section {
                "liveins" | "fixedStack" => {
                    for key in ["reg: '", "offset: ", "size: "] {
                        if let Some(v) = line.split(key).nth(1) {
                            lines.push(format!(
                                "{section} {key}{}",
                                v.split([',', '\'']).next().unwrap_or("")
                            ));
                        }
                    }
                }
                _ => {}
            }
            let inst = line.trim();
            if !(inst.starts_with("CALL64")
                || inst.starts_with("TCRETURN")
                || inst.starts_with("RET"))
            {
                continue;
            }
            let kept: Vec<&str> = inst
                .split(", ")
                .filter(|op| {
                    (op.starts_with("implicit ") || op.starts_with("implicit-def "))
                        && !op.ends_with("$rsp")
                        && !op.ends_with("$ssp")
                })
                .collect();
            lines.push(format!(
                "{} {}",
                inst.split_whitespace().next().unwrap_or(""),
                kept.join(" ")
            ));
        }
        out.insert(name.to_owned(), lines);
    }
    out
}
