//! WP-208: 512-bit vectors and vectors of `i1` under AVX-512
//! (`spec/abi-x64.md` §1.3, §3.2, §4.3, §5.1, §8.7, §9.1, §11). `qemu`'s
//! TCG has no AVX-512, so nothing compiled for these machines runs here:
//! the evidence is that hod's assignment of every signature of the grid
//! is, part for part, the convention `llc` 19 and 22 (`-O0`, `FastISel`
//! and `SelectionDAG`) give the same functions for the same machine,
//! read from their MIR after instruction selection: the registers a
//! definition receives its arguments in and returns its value in, the
//! stack slots it reads its arguments from, and the registers and stack
//! slots each call passes and receives. With `HOD_X64_AVX512_RUNNER` set
//! (a command that runs an x86-64 program on a CPU with AVX-512, the
//! program's path appended), or on an x86-64 processor whose flags have
//! the machine's features ([`required_features`], WP-229), the grid also
//! runs, as WP-203's does.

use std::collections::{BTreeMap, BTreeSet};

use hod_cg::abi::{Assignment, PartKind, Place};
use hod_testutil::{Scratch, run};
use hod_x64::abi::Features;
use hod_x64::reg;

use crate::grid::{pressures, run_cases};
use crate::ir;
use crate::run::{Machine, Tools};
use crate::sigs::{Attr, Blobs, Case, Plan, byval, p, pa, table};
use crate::ty::{I1, I8, I16, I32, I64, Rng, Ty, arr, st, vec};
use crate::wide::census;

/// The machines of the grid: every way `llc` decides that 512-bit
/// vectors are legal or not (§1.3): AVX-512 without VL; with VL, under a
/// CPU or tuning that prefers 256 bits and one that does not, with
/// `prefer-256-bit` set and cleared, with `min-legal-vector-width` 0, 256
/// and 512 and without it; with and without BW.
pub fn machines() -> Vec<Machine> {
    let with = |mut m: Machine, tune: Option<&str>, width: Option<u32>| {
        m.tune = tune.map(str::to_owned);
        m.min_legal = width;
        m
    };
    let base = |features: &str| Machine::new("x86-64", features);
    let fvlbw = "+avx512f,+avx512vl,+avx512bw,+avx512dq";
    vec![
        base("+avx512f"),
        with(base("+avx512f"), None, Some(0)),
        with(base(fvlbw), None, Some(0)),
        with(base(fvlbw), None, Some(512)),
        with(base(&format!("{fvlbw},+prefer-256-bit")), None, Some(0)),
        with(base(&format!("{fvlbw},+prefer-256-bit")), None, Some(256)),
        with(base(&format!("{fvlbw},+prefer-256-bit")), None, Some(512)),
        with(base("+avx512f,+avx512vl,+prefer-256-bit"), None, Some(0)),
        with(base("+avx512f,+avx512vl,+avx512bw,-prefer-256-bit"), None, Some(0)),
        census("skylake-avx512"),
        with(census("skylake-avx512"), None, Some(0)),
        with(census("skylake-avx512"), None, Some(512)),
        with(census("skylake-avx512"), Some("generic"), Some(0)),
        with(census("skylake-avx512"), None, Some(0)).minus_bw(),
        with(census("znver4"), None, Some(0)),
        with(census("sapphirerapids"), None, Some(0)),
        with(census("x86-64-v4"), None, Some(0)),
        with(Machine::new("knl", ""), None, Some(0)),
    ]
}

trait MinusBw {
    fn minus_bw(self) -> Machine;
}

impl MinusBw for Machine {
    fn minus_bw(mut self) -> Machine {
        self.features.push_str(",-avx512bw");
        self
    }
}

/// The vectors whose travel AVX-512 decides (§4.3): of 512 bits and
/// more, widened to 512 bits, and of `i1` of every length up to 64 (and
/// beyond, refused).
pub fn zmm_vectors() -> Vec<Ty> {
    let (h, b, f, d) = (Ty::Half, Ty::BFloat, Ty::Float, Ty::Double);
    let shapes: [(Ty, &[u32]); 10] = [
        (I1, &[1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 24, 31, 32, 33, 48, 63, 64, 65, 128]),
        (I8, &[33, 48, 64, 96, 128]),
        (I16, &[17, 24, 32, 64]),
        (I32, &[9, 10, 12, 16, 24, 32]),
        (I64, &[5, 6, 8, 16]),
        (Ty::Ptr, &[6, 8]),
        (h, &[24, 32]),
        (b, &[32]),
        (f, &[9, 12, 13, 16, 17, 32]),
        (d, &[5, 6, 8, 16]),
    ];
    let mut all = Vec::new();
    for (elem, lens) in shapes {
        all.extend(lens.iter().map(|&len| vec(elem.clone(), len)));
    }
    all
}

/// The aggregates holding one such vector, and some with more.
fn zmm_aggregates() -> Vec<Ty> {
    let (v16f32, v8f64, v64i8) = (vec(Ty::Float, 16), vec(Ty::Double, 8), vec(I8, 64));
    vec![
        st(std::slice::from_ref(&v16f32)),
        st(std::slice::from_ref(&v8f64)),
        arr(v64i8.clone(), 1),
        st(&[arr(v16f32.clone(), 1)]),
        st(&[v16f32.clone(), v16f32.clone()]),
        st(&[v8f64.clone(), I32]),
        st(&[vec(I1, 32)]),
        st(&[vec(I1, 64)]),
        st(&[vec(I1, 16), I8]),
        arr(vec(I1, 8), 2),
    ]
}

/// The cases: every type under every pressure (argument, return,
/// variadic), the aggregates, returns sharing `xmm0`..`xmm3`, `sret` and
/// `byval` around them, many vector registers.
pub fn zmm_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut types = zmm_vectors();
    types.extend(zmm_aggregates());
    for ty in &types {
        cases.extend(pressures(ty));
    }
    let (v16f32, v8f64, v8f32) = (vec(Ty::Float, 16), vec(Ty::Double, 8), vec(Ty::Float, 8));
    let (v4f32, d) = (vec(Ty::Float, 4), Ty::Double);
    let (m32, m64, m5) = (vec(I1, 32), vec(I1, 64), vec(I1, 5));
    cases.push(
        Case::of(&[v16f32.clone(), v8f32.clone(), v4f32.clone(), m32.clone()]).ret(v8f64.clone()),
    );
    cases.push(
        Case::of(&[m64.clone(), m32.clone(), v16f32.clone(), m5.clone(), I8]).ret(m64.clone()),
    );
    for ret in [
        st(&[v16f32.clone(), v4f32.clone()]),
        st(&[v16f32.clone(), v8f32.clone(), v16f32.clone(), d.clone()]),
        st(&[v16f32.clone(), v16f32.clone(), v16f32.clone(), v16f32.clone()]),
        st(&[v16f32.clone(), v16f32.clone(), v16f32.clone(), v16f32.clone(), Ty::Float]),
        vec(Ty::Float, 128),
        m5.clone(),
        vec(I1, 3),
        vec(I1, 48),
    ] {
        cases.push(Case::of(&[v16f32.clone(), I16]).ret(ret));
    }
    let many: Vec<Ty> =
        std::iter::repeat_n(v16f32.clone(), 9).chain([I32, v8f64.clone()]).collect();
    cases.push(Case::of(&many).ret(v16f32.clone()));
    cases.push(Case::new(vec![pa(Ty::Ptr, Attr::Sret), p(v16f32.clone()), p(m32.clone()), p(I32)]));
    for pointee in [v16f32.clone(), st(&[v8f64.clone(), v8f64.clone()])] {
        cases.push(Case::new(vec![byval(pointee.clone(), Some(64), None), p(v8f64.clone())]));
        cases.push(Case::new(vec![p(I32), byval(pointee, Some(64), None), p(I8)]).variadic(1));
    }
    cases.push(Case::of(&[I32, v16f32.clone(), v8f64.clone(), d.clone(), m32.clone()]).variadic(1));
    cases.push(Case::of(&[v16f32.clone(), I32, m64.clone(), m5, v8f32]).variadic(2).ret(v16f32));
    cases
}

/// The cases `assign` accepts under `features`, and the names of those
/// it refuses.
fn accepted(cases: Vec<Case>, features: Features) -> (Vec<Case>, BTreeMap<&'static str, usize>) {
    let mut types = table();
    let mut refused = BTreeMap::new();
    let mut kept: Vec<Case> = Vec::new();
    for case in cases {
        match case.assign_with(&mut types, features) {
            Ok(_) if !kept.contains(&case) => kept.push(case),
            Ok(_) => {}
            Err(name) => *refused.entry(name).or_insert(0) += 1,
        }
    }
    (kept, refused)
}

/// §1.3, §3.2, §4.3, §5.1, §8.7, §9.1, §11 (WP-208): the grid on every
/// machine of [`machines`]: every signature hod accepts is placed as each
/// `llc` places it ([`same_as_llc`]); hod refuses only a mask of more
/// than 64 elements (`x64:mask-vector`), as it refuses one without
/// AVX-512. With `HOD_X64_AVX512_RUNNER`, or natively on a processor with
/// the machine's features ([`avx512_runner`]), the signatures the driver
/// generator makes ([`generated`]) also run; each machine's outcome is
/// reported with their number (`HOD_AVX512_REPORT`).
#[test]
fn zmm_grid() {
    let Some(base) = Tools::find() else { return };
    let mut total = 0;
    for (n, machine) in machines().into_iter().enumerate() {
        let features = machine.abi();
        assert!(features.avx512 && features.avx512_known, "{}: {features:?}", machine.label());
        let (cases, refused) = accepted(zmm_cases(), features);
        for name in refused.keys() {
            assert_eq!(*name, "x64:mask-vector", "{} refused as {name}", machine.label());
        }
        let checked = same_as_llc(&format!("abi-zmm{n}"), &cases, &machine, &base);
        total += cases.len();
        eprintln!(
            "zmm grid, {}: zmm {} bw {}: {} signatures, {checked} llc conventions equal; \
             refused: {refused:?}",
            machine.label(),
            features.zmm,
            features.bw,
            cases.len()
        );
        // The number: two labels can be equal (the label cuts the
        // feature string short).
        let name = format!("abi zmm_grid {n} {}", machine.label());
        if let Some((tools, how)) = avx512_runner(&base, &machine, &name) {
            // Every accepted signature runs (WP-238).
            let how = format!("{how}; {} signatures", cases.len());
            eprintln!("zmm grid run {n}, {}: starting ({how})", machine.label());
            // The time is printed, for the cost of a run under an
            // emulator; nothing depends on it.
            let start = std::time::Instant::now();
            let report = run_cases(&format!("abi-zmm-run{n}"), &cases, 0x208, &tools);
            eprintln!(
                "zmm grid run {n}, {} ({how}; {:.0?}): {}",
                machine.label(),
                start.elapsed(),
                report.summary()
            );
            if !report.failures.is_empty() {
                let first = report.failures.first().map_or("", String::as_str);
                let detail = format!("{how}; {} failure(s); {first}", report.failures.len());
                hod_testutil::avx512::report(&name, "failed", &detail);
            }
            report.check(&format!("zmm grid {} ({how})", machine.label()));
            let detail = format!("{how}; {}", report.summary());
            hod_testutil::avx512::report(&name, "executed", &detail);
        }
    }
    eprintln!("zmm grid: {total} signatures over {} machines", machines().len());
}

/// WP-238: the driver generator (`emit.rs`) makes every signature the
/// grid accepts on every machine, which it did not before: a 64-byte part
/// in a `zmm` or on the stack, a mask in the byte lanes of a `ymm`, a
/// `zmm` or two `ymm`, and a mask passed a byte per element in general
/// registers and stack slots (§4.3). Each batch is generated and
/// assembled with its driver, not run (no emulator here has AVX-512):
/// what [`zmm_grid`] runs where a processor or a runner has AVX-512. The
/// grid has every one of those shapes.
#[test]
fn generator_makes_every_signature() {
    let Some(base) = Tools::find() else { return };
    let mut shapes = BTreeSet::new();
    for (n, machine) in machines().into_iter().enumerate() {
        let features = machine.abi();
        let (cases, _) = accepted(zmm_cases(), features);
        let mut types = table();
        for case in &cases {
            let asg = case.assign_with(&mut types, features).expect("accepted");
            for part in asg.args.iter().chain(&asg.ret) {
                let reg = match part.place {
                    Place::Reg(reg) if reg::is_gpr(reg) => "gpr",
                    Place::Reg(_) => "vector",
                    Place::Stack(_) => "stack",
                };
                let lanes = matches!(part.kind, PartKind::Lanes { bits: 1, .. });
                if lanes || part.size > 32 {
                    shapes.insert((lanes, part.size, reg));
                }
            }
        }
        let tools = base.with_machine(machine).expect("the tools");
        for (number, chunk) in cases.chunks(crate::grid::BATCH).enumerate() {
            crate::run::Batch::new(&format!("abi-zmm-gen{n}"), chunk, number as u64, &tools);
        }
    }
    for shape in [
        (false, 64, "vector"),
        (false, 64, "stack"),
        (true, 32, "vector"),
        (true, 64, "vector"),
        (true, 1, "gpr"),
        (true, 1, "stack"),
    ] {
        assert!(shapes.contains(&shape), "the grid has no part {shape:?}: {shapes:?}");
    }
}

/// The features a program compiled for `machine` may use that an x86-64
/// processor with AVX-512 may lack, in LLVM's names: the vector ones the
/// machine's feature string enables (`avx*` but AVX10, `fma`, `f16c`,
/// `gfni`, `vaes`, `vpclmulqdq`, `bmi`, `bmi2`) and not disables
/// afterwards, and Knights Landing's own (`knl`, whose string is empty).
/// A census CPU's string lists every feature of the CPU.
pub fn required_features(machine: &Machine) -> Vec<String> {
    let vector = |name: &str| {
        (name.starts_with("avx") && !name.starts_with("avx10"))
            || ["fma", "f16c", "gfni", "vaes", "vpclmulqdq", "bmi", "bmi2"].contains(&name)
    };
    let mut required: Vec<String> = Vec::new();
    if machine.cpu == "knl" {
        required.extend(["avx512f", "avx512cd", "avx512er", "avx512pf"].map(String::from));
    }
    for item in machine.features.split(',') {
        if let Some(name) = item.strip_prefix('+').filter(|n| vector(n)) {
            if !required.iter().any(|r| r == name) {
                required.push(name.to_owned());
            }
        } else if let Some(name) = item.strip_prefix('-') {
            required.retain(|r| r != name);
        }
    }
    required
}

/// The tools that run the grid of `machine` (§11), and how they run it,
/// as `hod_testutil::avx512` decides (as for
/// `x86_avx512::avx512_names_agree_with_llc_on_hardware`): with
/// `HOD_X64_AVX512_RUNNER` as the runner (`native`: the host itself),
/// else natively on an x86-64 processor with every feature of
/// [`required_features`]; `None`, reported under `name`, when the grid is
/// not run.
pub fn avx512_runner(base: &Tools, machine: &Machine, name: &str) -> Option<(Tools, String)> {
    use hod_testutil::avx512::{self, Execution};
    let required = required_features(machine);
    let required: Vec<&str> = required.iter().map(String::as_str).collect();
    let (prefix, how) = match avx512::execution(&required) {
        Execution::Run { prefix, how } => (prefix, how),
        Execution::DryRun(why) => {
            avx512::skip(name, "dry-run", &format!("{why}; requires {}", required.join(",")));
            return None;
        }
        Execution::Skip(why) => {
            avx512::skip(name, "skipped", &why);
            return None;
        }
    };
    let mut tools = base.with_machine(machine.clone())?;
    tools.runner = prefix.into_iter().map(Into::into).collect();
    Some((tools, how))
}

/// WP-229: what each machine of the grid requires of a processor to run
/// natively, and that a captured Xeon's flags (Ice Lake server: F, CD,
/// VL, BW, DQ, VBMI, VBMI2, no FP16, no BF16) select the right machines.
#[test]
fn zmm_machines_require_their_features() {
    let all = machines();
    let required: Vec<Vec<String>> = all.iter().map(required_features).collect();
    let has = |n: usize, f: &str| required[n].iter().any(|r| r == f);
    assert_eq!(required[0], ["avx512f"]);
    assert_eq!(required[2], ["avx512f", "avx512vl", "avx512bw", "avx512dq"]);
    // Tuning is not required; a disabled feature is not either.
    assert!(required.iter().flatten().all(|r| !r.starts_with("prefer") && r != "evex512"));
    assert_eq!(required[7], ["avx512f", "avx512vl"]);
    assert_eq!(required[8], ["avx512f", "avx512vl", "avx512bw"]);
    let label = |n: usize| all[n].label();
    assert!(label(13).starts_with("skylake-avx512") && has(12, "avx512bw") && !has(13, "avx512bw"));
    assert!(label(15).starts_with("sapphirerapids") && has(15, "avx512fp16"));
    assert!(label(14).starts_with("znver4") && has(14, "avx512bf16") && !has(14, "avx512fp16"));
    assert_eq!(required[17], ["avx512f", "avx512cd", "avx512er", "avx512pf"]);
    let xeon = hod_testutil::avx512::CpuInfo::parse(
        "model name : Xeon\nflags : avx avx2 fma f16c bmi1 bmi2 avx512f avx512dq avx512ifma \
         avx512cd avx512bw avx512vl avx512vbmi avx512_vbmi2 gfni vaes vpclmulqdq avx512_vnni \
         avx512_bitalg avx512_vpopcntdq\n",
    )
    .unwrap();
    let runs: Vec<usize> = (0..all.len())
        .filter(|&n| {
            let r: Vec<&str> = required[n].iter().map(String::as_str).collect();
            xeon.missing(&r).is_empty()
        })
        .collect();
    // All but znver4 (BF16), sapphirerapids (FP16, BF16) and knl (ER, PF).
    assert_eq!(runs, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16]);
}

/// For each case, a definition receiving it (`lf_<n>`, not for a
/// variadic one: LLVM has no `va_arg` here) and a call making it
/// (`lc_<n>`), compiled by each `llc` at `-O0` for `machine`: the
/// convention read from the MIR ([`llc_conventions`]) equals hod's
/// ([`hod_convention`]). Returns how many were compared.
pub fn same_as_llc(label: &str, cases: &[Case], machine: &Machine, tools: &Tools) -> usize {
    let (compared, wrong) = differences(label, cases, machine, machine.abi(), tools);
    assert!(
        wrong.is_empty(),
        "{} conventions differ:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(10)].join("\n")
    );
    compared
}

/// [`same_as_llc`] with hod assigning under `features`: how many
/// conventions were compared, and those that differ.
fn differences(
    label: &str,
    cases: &[Case],
    machine: &Machine,
    features: Features,
    tools: &Tools,
) -> (usize, Vec<String>) {
    let mut types = table();
    let (mut blobs, mut rng) = (Blobs::default(), Rng(0x208));
    let plans: Vec<Plan> = cases
        .iter()
        .map(|case| Plan::new_with(case, &mut types, &mut blobs, &mut rng, features))
        .collect();
    let runs = |index: usize, hod_calls: bool| !(hod_calls && plans[index].case.fixed.is_some());
    // LLVM 19's FastISel mishandles a `<32 x bfloat>` where `bf16` is
    // legal: it drops it as a call's argument (`znver4`; no `zmm0`, the
    // next `xmm` argument takes `xmm0`) or runs out of memory
    // (`sapphirerapids`). Its SelectionDAG and LLVM 22 pass it in `zmm0`,
    // as hod does. Those cases are left out of the module it compiles.
    let bf16 = |index: usize| plans[index].case.text().contains("<32 x bfloat>");
    let runs19 = |index: usize, hod_calls: bool| runs(index, hod_calls) && !bf16(index);
    let scratch = Scratch::new(label);
    let attributes = machine.attributes();
    let source = scratch.write("grid.ll", ir::module_with(&plans, &runs, Some(&attributes)));
    let source19 = scratch.write("grid19.ll", ir::module_with(&plans, &runs19, Some(&attributes)));
    let mut compared = 0;
    let mut wrong = Vec::new();
    for (config, llc) in tools.llc.iter().filter(|(config, _)| config.opt == "-O0") {
        let fast19 = config.major < 22 && !config.dag;
        let source = if fast19 { &source19 } else { &source };
        // A grid module holds two functions per case: more than the
        // default minute for `llc` 19 on some machines.
        let outcome = hod_testutil::run_with(
            std::process::Command::new(llc).args([
                "-O0".as_ref(),
                if config.dag { "-fast-isel=0" } else { "-fast-isel=1" }.as_ref(),
                "-relocation-model=pic".as_ref(),
                "-stop-after=finalize-isel".as_ref(),
                "-o".as_ref(),
                "-".as_ref(),
                source.as_os_str(),
            ]),
            &[],
            std::time::Duration::from_mins(10),
        );
        assert!(
            outcome.success(),
            "llc {} ({:?}): {}",
            config.tag(),
            outcome.status,
            outcome.stderr_text()
        );
        let mir = outcome.stdout_text();
        let theirs = llc_conventions(&mir, "@hf_");
        for (index, plan) in plans.iter().enumerate() {
            for (callee, name) in [(true, format!("lf_{index}")), (false, format!("lc_{index}"))] {
                // As the grid (`run::runs`): LLVM 19 keeps a value split
                // into general pieces whole, §4.2.
                let llvm19 = config.major < 22
                    && plan.case.has_wide_general()
                    && (plan.asg.stack_size > 0 || plan.case.fixed.is_some());
                if !runs(index, callee) || llvm19 || (fast19 && bf16(index)) {
                    continue;
                }
                let ours = hod_convention(&plan.asg, callee, plan.case.fixed.is_some());
                let got = theirs.get(&name).map(|c| c.normalized(&plan.asg, callee));
                if got.as_ref() != Some(&ours) {
                    wrong.push(format!(
                        "llc {} {}: `{name}` `{}`:\n  hod {ours:?}\n  llc {got:?}",
                        config.tag(),
                        machine.label(),
                        plan.case.text()
                    ));
                }
                compared += 1;
            }
        }
    }
    (compared, wrong)
}

/// A register by the name of its 64-bit or vector register: `rdi` for
/// `dil`..`rdi`, `xmm3/32` for `ymm3`, `st0` for `fp0`.
pub fn canonical(name: &str) -> String {
    const GPRS: [(&str, [&str; 4]); 9] = [
        ("rax", ["al", "ax", "eax", "rax"]),
        ("rcx", ["cl", "cx", "ecx", "rcx"]),
        ("rdx", ["dl", "dx", "edx", "rdx"]),
        ("rsi", ["sil", "si", "esi", "rsi"]),
        ("rdi", ["dil", "di", "edi", "rdi"]),
        ("r8", ["r8b", "r8w", "r8d", "r8"]),
        ("r9", ["r9b", "r9w", "r9d", "r9"]),
        ("r10", ["r10b", "r10w", "r10d", "r10"]),
        ("r11", ["r11b", "r11w", "r11d", "r11"]),
    ];
    for (big, names) in GPRS {
        if names.contains(&name) {
            return big.to_owned();
        }
    }
    for (prefix, bytes) in [("xmm", 16), ("ymm", 32), ("zmm", 64)] {
        if let Some(n) = name.strip_prefix(prefix) {
            return format!("xmm{n}/{bytes}");
        }
    }
    if let Some(n) = name.strip_prefix("fp") {
        return format!("st{n}");
    }
    name.to_owned()
}

/// The name of hod's register `reg` for a part of `size` bytes, as
/// [`canonical`] writes it.
fn hod_reg(place: hod_cg::PReg, size: u32) -> String {
    if reg::is_gpr(place) {
        return canonical(&format!("{:?}", reg::reg(place)).to_lowercase());
    }
    if reg::is_x87(place) {
        return format!("st{}", reg::number(place));
    }
    let bytes = match size {
        0..=16 => 16,
        17..=32 => 32,
        _ => 64,
    };
    format!("{}/{bytes}", format!("{:?}", reg::xreg(place)).to_lowercase())
}

/// hod's convention of a signature assigned `asg`, as a definition
/// receiving it (`callee`) or a call making it (`variadic`: a variadic
/// call): `arg <reg>` and `stk <offset>` for each part of an argument,
/// `ret <reg>` for each part of the return value, `arg al` for a variadic
/// call.
pub fn hod_convention(asg: &Assignment, callee: bool, variadic: bool) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for part in &asg.args {
        // A call stores nothing for a zero-sized `byval`; a definition
        // still has its fixed stack object.
        if !callee && part.size == 0 && matches!(part.place, Place::Stack(_)) {
            continue;
        }
        out.insert(match part.place {
            Place::Reg(r) => format!("arg {}", hod_reg(r, part.size)),
            Place::Stack(offset) => format!("stk {offset}"),
        });
    }
    if let Some(memory) = &asg.ret_memory
        && let Place::Reg(r) = memory.address
    {
        out.insert(format!("arg {}", hod_reg(r, 8)));
    }
    let returned = asg.ret_memory.and_then(|m| m.returned_in).or(asg.sret_returned_in);
    if let (true, Some(r)) = (callee, returned) {
        out.insert(format!("ret {}", hod_reg(r, 8)));
    }
    for part in &asg.ret {
        if let Place::Reg(r) = part.place {
            out.insert(format!("ret {}", hod_reg(r, part.size)));
        }
    }
    if !callee && variadic {
        out.insert("arg al".to_owned());
    }
    out
}

/// One function's convention as `llc`'s MIR has it: as a definition,
/// its live-in registers, the registers of its return and its incoming
/// stack objects; as a caller, the registers of its calls and
/// the stores to the outgoing argument area.
#[derive(Default, Debug)]
pub struct LlcConvention {
    def: BTreeSet<String>,
    call: BTreeSet<String>,
    /// Offset of each fixed stack object.
    fixed: Vec<u32>,
    /// Offset of each store to the outgoing argument area.
    stores: Vec<u32>,
}

impl LlcConvention {
    /// The convention in [`hod_convention`]'s terms: a stack offset is
    /// the offset of the part of `asg` it falls in (`?<offset>` when it
    /// falls in none).
    pub fn normalized(&self, asg: &Assignment, callee: bool) -> BTreeSet<String> {
        let (mut out, stack) = if callee {
            (self.def.clone(), &self.fixed)
        } else {
            (self.call.clone(), &self.stores)
        };
        for &offset in stack {
            let part = asg.args.iter().find_map(|part| match part.place {
                Place::Stack(at) if at <= offset && offset < at + part.size.max(8) => Some(at),
                _ => None,
            });
            out.insert(part.map_or(format!("stk ?{offset}"), |at| format!("stk {at}")));
        }
        out
    }
}

/// Each function's [`LlcConvention`] in `mir`, its calls those of the
/// functions whose name starts with `callee` (`@` and the prefix).
pub fn llc_conventions(mir: &str, callee: &str) -> BTreeMap<String, LlcConvention> {
    let mut out = BTreeMap::new();
    for doc in mir.split("\n---\n") {
        let Some(name) = doc.lines().find_map(|l| l.strip_prefix("name:")).map(str::trim) else {
            continue;
        };
        let mut c = LlcConvention::default();
        let mut section = "";
        // Virtual registers holding `rsp` plus an offset (the
        // destination of a `byval` copy), and the destination in `rdi`
        // of a block copy to come.
        let mut on_rsp: BTreeMap<String, u32> = BTreeMap::new();
        let mut rdi_at = None;
        for line in doc.lines() {
            if !line.starts_with(' ') {
                section = line.split(':').next().unwrap_or("");
                continue;
            }
            let field = |key: &str| {
                let value = line.split(key).nth(1)?;
                value.split([',', '\'', ' ']).next().map(str::to_owned)
            };
            match section {
                "liveins" => {
                    if let Some(r) = field("reg: '$") {
                        c.def.insert(format!("arg {}", canonical(&r)));
                    }
                    continue;
                }
                "fixedStack" => {
                    if let Some(o) = field("offset: ") {
                        c.fixed.push(o.parse().expect("an offset"));
                    }
                    continue;
                }
                _ => {}
            }
            let inst = line.trim();
            let words: Vec<&str> = inst.split([' ', ',']).filter(|w| !w.is_empty()).collect();
            let base = |w: &str| if w == "$rsp" { Some(0) } else { on_rsp.get(w).copied() };
            let number = |w: Option<&&str>| w.and_then(|d| d.parse::<u32>().ok()).unwrap_or(0);
            if inst.starts_with("RET") {
                // Before the x87 stackifier an `fp` result is a virtual
                // register: `st0`, then `st1`.
                let mut x87 = 0;
                for w in &words {
                    if let Some(r) = w.strip_prefix('$') {
                        c.def.insert(format!("ret {}", canonical(r)));
                    } else if w.starts_with('%') {
                        c.def.insert(format!("ret st{x87}"));
                        x87 += 1;
                    }
                }
            } else if let [def, "=", op, from, rest @ ..] = &words[..]
                && def.starts_with('%')
                && let Some(at) = base(from)
                && matches!(*op, "COPY" | "ADD64ri32" | "ADD64ri8" | "LEA64r")
            {
                // `rsp`, `rsp + d` (an `add`, or a `lea` with base,
                // scale, index, displacement).
                let d = match *op {
                    "COPY" => 0,
                    "LEA64r" => number(rest.get(2)),
                    _ => number(rest.first()),
                };
                on_rsp.insert(def.split(':').next().unwrap_or("").to_owned(), at + d);
            } else if let ["$rdi", "=", "COPY", from] = &words[..] {
                rdi_at = base(from);
            } else if inst.starts_with("REP_MOVS") || inst.contains("memcpy") {
                // A block copy into the argument area.
                c.stores.extend(rdi_at.take());
            } else if words.first().is_some_and(|op| op.ends_with("mr") || op.ends_with("mi"))
                && let Some(at) = words.get(1).and_then(|w| base(w))
            {
                // A store through `rsp` or a register holding `rsp`
                // plus an offset: base, scale, index, displacement.
                c.stores.push(at + number(words.get(4)));
            } else if inst.starts_with("CALL64") && inst.contains(callee) {
                for (i, w) in words.iter().enumerate() {
                    let Some(r) = w.strip_prefix('$') else { continue };
                    if matches!(r, "rsp" | "ssp") {
                        continue;
                    }
                    let def = words[..i].iter().rev().find(|w| w.starts_with("implicit"));
                    c.call.insert(match (def == Some(&"implicit-def"), r) {
                        (true, r) => format!("ret {}", canonical(r)),
                        (false, "al") => "arg al".to_owned(),
                        (false, r) => format!("arg {}", canonical(r)),
                    });
                }
            } else if let Some(at) = inst.split(" into stack").nth(1) {
                let offset = at.strip_prefix(" + ").map_or(0, |v| {
                    v.split([',', ')', ' ']).next().unwrap_or("0").parse().unwrap_or(0)
                });
                c.stores.push(offset);
            }
        }
        out.insert(name.to_owned(), c);
    }
    out
}

/// The comparison bites: a `<16 x float>` hod assigns for a machine on
/// which 512-bit vectors are legal (one `zmm`) is not what `llc` does on
/// one that prefers 256 bits (two `ymm`), and the reverse; each agrees
/// with `llc` on its own machine.
#[test]
fn conventions_tell_zmm_from_two_ymm() {
    let Some(tools) = Tools::find() else { return };
    let case = Case::of(&[vec(Ty::Float, 16), I16]).ret(vec(Ty::Float, 16));
    let wide = Machine::new("x86-64", "+avx512f");
    let mut narrow = Machine::new("x86-64", "+avx512f,+avx512vl,+prefer-256-bit");
    narrow.min_legal = Some(0);
    let cases = std::slice::from_ref(&case);
    for (m, features, differ) in [
        (&wide, wide.abi(), false),
        (&narrow, narrow.abi(), false),
        (&wide, narrow.abi(), true),
        (&narrow, wide.abi(), true),
    ] {
        // A definition and a call under each `-O0` `llc`.
        let (compared, wrong) = differences("abi-zmm-bite", cases, m, features, &tools);
        let expected = if differ { compared } else { 0 };
        assert!(compared >= 2, "{}: nothing compared", m.label());
        assert_eq!(wrong.len(), expected, "{}: {wrong:#?}", m.label());
    }
}

// --- what clang 19 and 22 write -------------------------------------------------

/// C++ that passes and returns 512-bit vectors, classes of one, masks
/// (`bool` vectors), a vector widened to 512 bits, more than eight
/// vector arguments, a variadic callee reading `__m512` with `va_arg`,
/// and a class wider than 128 bits whose base is narrower than itself
/// (which clang 21 and later pass in memory, clang 19 in registers);
/// no element of a mask is assigned (clang 22 writes `insertelement` of an
/// `i32` into `<N x i1>` for that, which its own `llc` rejects):
/// each `d*` defined, each `x*` declared and called by `calls`.
const CLANG_CXX: &str = r#"
#include <immintrin.h>
#include <cstdarg>
typedef bool m5 __attribute__((ext_vector_type(5)));
typedef bool m8 __attribute__((ext_vector_type(8)));
typedef bool m32 __attribute__((ext_vector_type(32)));
typedef bool m64 __attribute__((ext_vector_type(64)));
typedef float v12 __attribute__((ext_vector_type(12)));
struct Z { __m512 v; };
struct ZD { __m512d v; };
union UZ { __m512i v; long long q[8]; };
struct B { __m256 a; };
struct D : B { __m256 b; };
struct Z2 { __m512 a, b; };
#define SIGS(p) \
  __m512 p##1(__m512 a, __m512d b, __m512i c, int k, __m128 x, __m256 y); \
  Z p##2(Z z, ZD d, int k); \
  UZ p##3(UZ u); \
  D p##4(D d, __m512 v); \
  Z2 p##5(Z2 z); \
  m32 p##6(m32 a, m64 b, m8 c, m5 d, m5 e, m5 f); \
  v12 p##7(v12 a, __m512 b); \
  __m512 p##8(__m512 a0, __m512 a1, __m512 a2, __m512 a3, __m512 a4, __m512 a5, \
              __m512 a6, __m512 a7, __m512 a8, int k, __m512 a9); \
  m64 p##9(m64 a); \
  double p##v(int n, ...);
extern "C" {
SIGS(x)
SIGS(d)
__m512 d1(__m512 a, __m512d b, __m512i c, int k, __m128 x, __m256 y) {
  return a + (__m512)b + (__m512)c + (float)k + x[1] + y[2];
}
Z d2(Z z, ZD d, int k) { z.v[0] += d.v[1] + k; return z; }
UZ d3(UZ u) { u.q[7] += 1; return u; }
D d4(D d, __m512 v) { d.b[0] += v[3]; d.a[1] -= 1; return d; }
Z2 d5(Z2 z) { Z2 r = { z.b, z.a }; return r; }
m32 d6(m32 a, m64 b, m8 c, m5 d, m5 e, m5 f) { return b[60] ^ c[7] ^ d[4] ^ e[0] ^ f[2] ? a : m32{}; }
v12 d7(v12 a, __m512 b) { a[11] += b[15]; return a; }
__m512 d8(__m512 a0, __m512 a1, __m512 a2, __m512 a3, __m512 a4, __m512 a5,
          __m512 a6, __m512 a7, __m512 a8, int k, __m512 a9) {
  return a0 + a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 * (float)k - a9;
}
m64 d9(m64 a) { return a[0] ? m64{} : a; }
double dv(int n, ...) {
  va_list ap;
  va_start(ap, n);
  __m512 s = {};
  for (int i = 0; i < n; i++) s += va_arg(ap, __m512);
  double d = va_arg(ap, double);
  va_end(ap);
  return s[0] + 2 * s[15] + d;
}
double calls(__m512 a, __m512d b, __m512i c, Z z, ZD zd, UZ u, D d, Z2 z2, m32 k32, m64 k64,
             m8 k8, m5 k5, v12 w) {
  __m512 r = x1(a, b, c, 3, __m128{}, __m256{});
  r += x2(z, zd, 4).v;
  r[0] += x3(u).q[0];
  r[1] += x4(d, a).b[2];
  r += x5(z2).b;
  m32 m = x6(k32, k64, k8, k5, k5, k5);
  r[2] += m[1] + x9(k64)[5];
  r[3] += x7(w, a)[11];
  r += x8(a, a, a, a, a, a, a, a, a, 2, r);
  return r[0] + r[3] + xv(2, a, r, 1.5);
}
}
"#;

/// The `-march` and `-m` options of the clang comparison: AVX-512 with
/// and without VL and BW, CPUs that prefer 256 bits and one that does
/// not, and a preferred width set either way.
const CLANG_FLAGS: [&str; 7] = [
    "-mavx512f",
    "-mavx512f -mavx512vl -mavx512bw -mprefer-vector-width=256",
    "-march=skylake-avx512",
    "-march=skylake-avx512 -mprefer-vector-width=512",
    "-march=x86-64-v4",
    "-march=znver4",
    "-march=sapphirerapids",
];

/// §1.3, §4.3, §8.7 (WP-208): the C++ of [`CLANG_CXX`] as clang 19 and
/// 22 write it for each of [`CLANG_FLAGS`] (with the
/// `min-legal-vector-width` and `tune-cpu` they write): hod compiles every
/// module (plainly and under every stress option), and its convention
/// of every definition and of every call, under that function's own
/// attributes, is the one the `llc` of the same LLVM gives it (`-O0`,
/// `FastISel` and `SelectionDAG`). Each LLVM against its own clang: from
/// clang 21 on, a class wider than 128 bits whose base is narrower than
/// itself travels in memory (`D` here).
#[test]
fn clang_signatures_agree_with_their_llc() {
    use hod_cg::Target as _;
    let Some(tools) = Tools::find() else { return };
    let scratch = Scratch::new("abi-zmm-clang");
    let source = scratch.write("zmm.cpp", CLANG_CXX);
    let mut compared = 0;
    for major in [19, 22] {
        let Some(clang) = crate::run::llvm_tool(major, "clang") else { continue };
        for (n, flags) in CLANG_FLAGS.iter().enumerate() {
            let ll = scratch.path().join(format!("zmm{major}-{n}.ll"));
            let outcome = run(
                &clang,
                ["--target=x86_64-unknown-linux-gnu", "-O0", "-S", "-emit-llvm", "-std=c++20"]
                    .iter()
                    .map(std::ffi::OsStr::new)
                    .chain(flags.split_whitespace().map(std::ffi::OsStr::new))
                    .chain([source.as_os_str(), "-o".as_ref(), ll.as_os_str()]),
            );
            assert!(outcome.success(), "clang {major} {flags}: {}", outcome.stderr_text());
            let text = std::fs::read_to_string(&ll).expect("the module");
            let (module, bodies) = parse_ll(&text);
            for stress in [hod_cg::Stress::default(), hod_cg::Stress::ALL] {
                let options = hod_cg::Options {
                    reloc_model: hod_cg::RelocModel::Pic,
                    stress,
                    ..hod_cg::Options::default()
                };
                if let Err(e) = hod_x64::compile_to_object(&module, &bodies, &options) {
                    panic!("hod refuses what clang {major} writes for {flags}: {e}");
                }
            }
            // Each function's target, by name.
            let by_name: BTreeMap<String, (hod_x64::X64, &hod_ir::Body)> = bodies
                .iter()
                .map(|body| {
                    let name = String::from_utf8_lossy(module.symbol(body.symbol()).name());
                    let target = hod_x64::X64.for_function(&module, body).expect("x86-64");
                    (name.into_owned(), (target, body))
                })
                .collect();
            let assign = |target: &hod_x64::X64, callee: &str| {
                let sym = module.find_symbol(callee.as_bytes()).expect("the function");
                let function = module.symbol(sym);
                let hod_ir::SymbolKind::Function { cc, ret_attrs, param_attrs, .. } =
                    function.kind()
                else {
                    panic!("{callee} is no function");
                };
                let hod_ir::TypeKind::Function { ret, params, variadic } =
                    module.types.kind(function.ty())
                else {
                    panic!("{callee} has no function type");
                };
                let fixed = u32::try_from(params.len()).unwrap();
                let sig =
                    hod_cg::abi::Sig { cc, ret, ret_attrs, params, param_attrs, fixed, variadic };
                let mut out = Assignment::default();
                target
                    .assign(&module.types, &sig, &mut out)
                    .unwrap_or_else(|e| panic!("{callee}: {e:?}"));
                (out, variadic)
            };
            for (config, llc) in
                tools.llc.iter().filter(|(c, _)| c.opt == "-O0" && c.major == major)
            {
                let outcome = run(
                    llc,
                    [
                        "-O0".as_ref(),
                        if config.dag { "-fast-isel=0" } else { "-fast-isel=1" }.as_ref(),
                        "-relocation-model=pic".as_ref(),
                        "-stop-after=finalize-isel".as_ref(),
                        "-o".as_ref(),
                        "-".as_ref(),
                        ll.as_os_str(),
                    ],
                );
                assert!(outcome.success(), "llc {}: {}", config.tag(), outcome.stderr_text());
                let theirs = llc_conventions(&outcome.stdout_text(), "@x");
                let mut wrong = Vec::new();
                for (name, (target, _)) in &by_name {
                    let Some(llc_side) = theirs.get(name) else { continue };
                    if name.starts_with('d') && name.len() == 2 {
                        let (asg, variadic) = assign(target, name);
                        let ours = hod_convention(&asg, true, false);
                        let mut got = llc_side.normalized(&asg, true);
                        if variadic {
                            // `va_start` takes every argument register and
                            // the overflow area: the named arguments are
                            // among them, the return is the same.
                            let named = ours.iter().all(|line| got.contains(line));
                            got.retain(|line| {
                                !named || ours.contains(line) || line.starts_with("ret")
                            });
                        }
                        if got != ours {
                            wrong.push(format!("{name}: hod {ours:?}\n  llc {got:?}"));
                        }
                        compared += 1;
                    }
                    if name == "calls" {
                        // Every call's registers together.
                        let mut ours = BTreeSet::new();
                        for callee in ["x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "xv"] {
                            let (asg, variadic) = assign(target, callee);
                            ours.extend(
                                hod_convention(&asg, false, variadic)
                                    .into_iter()
                                    .filter(|line| !line.starts_with("stk")),
                            );
                        }
                        let got: BTreeSet<String> = llc_side.call.clone();
                        if got != ours {
                            wrong.push(format!("calls: hod {ours:?}\n  llc {got:?}"));
                        }
                        compared += 1;
                    }
                }
                assert!(
                    wrong.is_empty(),
                    "clang and llc {} {flags}: {}",
                    config.tag(),
                    wrong.join("\n")
                );
            }
        }
    }
    eprintln!("zmm clang signatures: {compared} conventions equal to their llc's");
}

/// A module and its bodies from `.ll` text.
fn parse_ll(text: &str) -> (hod_ir::Module, Vec<hod_ir::Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        let body =
            parsed.body.unwrap_or_else(|| panic!("hod_ll: unsupported {:?}", parsed.unsupported));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    (module, bodies)
}
