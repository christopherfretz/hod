//! The call group (WP-038, `spec/codegen.md` §5.5): calls, `invoke`,
//! `musttail`, landing pads, `resume`, variadic definitions and the
//! `va_*` intrinsics, run natively against gcc and `llc`.

#[path = "common/callgen.rs"]
mod callgen;
mod common;

use std::path::PathBuf;

use callgen::{Case, Place, Rng};
use common::linking::{Toolchain, exit_code};
use common::{assert_refused, emit, ll_header, llc, llc_major, parse, write_object};
use hod_a64::call;
use hod_cg::Options;
use hod_cg::abi::{Assignment, Sig};
use hod_ir::{Module, TypeKind};
use hod_testutil::Scratch;

/// Where each named argument of `c_k` arrives, by `assign` on module B's
/// declaration of it.
fn places(module: &Module, cases: &[Case]) -> Vec<Vec<Place>> {
    cases
        .iter()
        .map(|case| {
            let sym = module.find_symbol(format!("c_{}", case.k).as_bytes()).expect("c_k");
            let sym = module.symbol(sym);
            let TypeKind::Function { ret, params, variadic } = module.types.kind(sym.ty()) else {
                panic!("c_k is a function")
            };
            let params = params.to_vec();
            let attrs: Vec<_> = (0..params.len()).map(|i| sym.param_attrs(i)).collect();
            let sig = Sig {
                cc: hod_ir::CallConv::C,
                ret,
                ret_attrs: hod_ir::ParamAttrs::NONE,
                params: &params,
                param_attrs: &attrs,
                fixed: params.len() as u32,
                variadic,
            };
            let mut out = Assignment::default();
            hod_a64::abi::assign(&module.types, &sig, &mut out).expect("assign");
            let skip = usize::from(case.ret[2] == callgen::Ret::Sret);
            (0..case.c_params.len())
                .map(|i| {
                    let arg = (i + skip) as u32;
                    match out.args.iter().rev().find(|part| part.arg == arg).map(|p| p.place) {
                        Some(hod_cg::abi::Place::Stack(_)) => Place::Stack,
                        _ => Place::Reg,
                    }
                })
                .collect()
        })
        .collect()
}

/// Compiles module text with hod (or `llc` when `with_llc`).
fn object(dir: &Scratch, name: &str, text: &str, with_llc: bool) -> Option<PathBuf> {
    object_with(dir, name, text, with_llc, &Options::default())
}

fn object_with(
    dir: &Scratch,
    name: &str,
    text: &str,
    with_llc: bool,
    options: &Options,
) -> Option<PathBuf> {
    if with_llc {
        let pic = options.reloc_model == hod_cg::RelocModel::Pic;
        let flags: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
        return llc(dir, name, &format!("{}{text}", ll_header(llc_major())), flags);
    }
    let (module, bodies) = parse(&format!("{}{text}", ll_header(22)));
    let emitted = emit(&module, &bodies, options);
    Some(write_object(dir, &format!("{name}.o"), &emitted))
}

/// A file of `tests/data/call`.
fn data(name: &str) -> String {
    let path = format!("{}/tests/data/call/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Links `inputs` with every linker (only the first when `first_only`),
/// runs the program and requires exit 0 and `marker` in its output.
fn link_run_all(
    toolchain: &Toolchain,
    dir: &Scratch,
    name: &str,
    flags: &[&str],
    inputs: &[&std::path::Path],
    marker: &str,
    first_only: bool,
) {
    let linkers = if first_only { &toolchain.linkers[..1] } else { &toolchain.linkers[..] };
    for linker in linkers {
        let exe = dir.join(format!("{name}-{}", linker.name));
        let ran = toolchain.link_and_run(linker, flags, inputs, &exe);
        let out = ran.stdout_text();
        assert!(
            exit_code(&ran) == Some(0) && out.contains(marker),
            "{name} {}: exit {:?}\n{out}{}",
            linker.name,
            exit_code(&ran),
            ran.stderr_text()
        );
    }
}

/// How the two modules of a generated program are built.
struct Build {
    tag: &'static str,
    /// Modules A and B by `llc` rather than hod.
    a_llc: bool,
    b_llc: bool,
    options: Options,
    /// B is linked as a shared object (both modules position-independent).
    shared: bool,
    all_linkers: bool,
}

/// Generates `count` cases from `seed`, and runs them as each of
/// `builds`.
fn run_cases(label: &str, seed: u64, count: usize, builds: &[Build]) {
    let dir = Scratch::new(label);
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let mut rng = Rng(seed);
    let cases: Vec<Case> = (0..count).map(|k| callgen::random_case(&mut rng, k)).collect();
    let a = callgen::module_a(&cases, "");
    let b = callgen::module_b(&cases, "");
    let (module_b, _) = parse(&format!("{}{b}", ll_header(22)));
    let at = places(&module_b, &cases);
    let c = callgen::driver(&cases, &|k| at[k].clone());
    let driver = toolchain.compile_c(&dir, "driver", &c, &["-Wno-psabi", "-w", "-fPIC"]);
    let mut objects_a = Vec::new();
    for build in builds {
        let tag = build.tag;
        let Some(obj_a) = object_with(&dir, &format!("a-{tag}"), &a, build.a_llc, &build.options)
        else {
            return;
        };
        let Some(obj_b) = object_with(&dir, &format!("b-{tag}"), &b, build.b_llc, &build.options)
        else {
            return;
        };
        objects_a.push((tag, obj_a.clone()));
        let linkers =
            if build.all_linkers { &toolchain.linkers[..] } else { &toolchain.linkers[..1] };
        for linker in linkers {
            let exe = dir.join(format!("p-{tag}-{}", linker.name));
            let ran = if build.shared {
                let lib = dir.join(format!("libb-{tag}-{}.so", linker.name));
                toolchain.link_ok(linker, &["-shared"], &[&obj_b], &lib);
                let rpath = format!("-Wl,-rpath,{}", dir.path().display());
                toolchain.link_and_run(
                    linker,
                    &["-rdynamic", &rpath],
                    &[&driver, &obj_a, &lib],
                    &exe,
                )
            } else {
                toolchain.link_and_run(linker, &[], &[&driver, &obj_a, &obj_b], &exe)
            };
            let out = ran.stdout_text();
            if exit_code(&ran) != Some(0) || !out.contains("checked") {
                panic!(
                    "{label} {tag} {}: {out}{}\n{}",
                    linker.name,
                    ran.stderr_text(),
                    failing_cases(&out, &[&a, &b, &c])
                );
            }
        }
    }
    // Calls and function addresses take the relocations `llc` gives them
    // (`hod_cg::call_access`, `sym_access`).
    let find = |tag: &str| objects_a.iter().find(|(t, _)| *t == tag).map(|(_, p)| p.clone());
    if let (Some(hod), Some(llc)) = (find("HH-pic"), find("LH-pic")) {
        assert_eq!(call_relocs(&hod), call_relocs(&llc), "{label}: PIC call relocations");
    }
    if let (Some(hod), Some(llc)) = (find("HH"), find("LL")) {
        assert_eq!(call_relocs(&hod), call_relocs(&llc), "{label}: call relocations");
    }
}

/// The call and GOT relocations of an object, as (type, symbol) counts.
fn call_relocs(object: &std::path::Path) -> std::collections::BTreeMap<(String, String), usize> {
    let mut out = std::collections::BTreeMap::new();
    let Some(readelf) = hod_testutil::tool("llvm-readelf") else { return out };
    let ran = hod_testutil::run(readelf, [std::ffi::OsStr::new("-rW"), object.as_os_str()]);
    for line in ran.stdout_text().lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [_, _, kind, _, sym, ..] = fields[..] else { continue };
        let wanted = ["CALL26", "JUMP26", "ADR_GOT_PAGE", "LD64_GOT_LO12_NC", "ADR_PREL_PG_HI21"];
        if wanted.iter().any(|w| kind.ends_with(w)) && !sym.starts_with('.') {
            *out.entry((kind.to_owned(), sym.to_owned())).or_insert(0) += 1;
        }
    }
    out
}

/// The text of the cases the driver reports failed, from each source.
fn failing_cases(out: &str, sources: &[&str]) -> String {
    let mut text = String::new();
    let mut seen = Vec::new();
    for line in out.lines() {
        let Some(k) = line.strip_prefix("FAIL case ").and_then(|r| r.split(' ').next()) else {
            continue;
        };
        if seen.contains(&k) || seen.len() == 3 {
            continue;
        }
        seen.push(k);
        for source in sources {
            let mut inside = false;
            for line in source.lines() {
                let mine =
                    [format!("_{k}("), format!("_{k} =")].iter().any(|n| line.contains(n.as_str()));
                if mine
                    && (line.contains('{') || line.starts_with("declare") || line.starts_with('@'))
                {
                    inside = line.trim_end().ends_with('{');
                    text.push_str(line);
                    text.push('\n');
                } else if inside {
                    text.push_str(line);
                    text.push('\n');
                    inside = line != "}";
                }
            }
        }
    }
    text
}

#[test]
fn random_signatures_agree_with_gcc_and_llc() {
    // `HOD_CALL_SEED` and `HOD_CALL_COUNT` explore other cases.
    let var = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u64>().ok());
    let seed = var("HOD_CALL_SEED").unwrap_or(0x38_0001);
    let count = var("HOD_CALL_COUNT").map_or(300, |n| n as usize);
    let pic = Options { reloc_model: hod_cg::RelocModel::Pic, ..Options::default() };
    let stress = Options { stress: hod_cg::Stress::ALL, ..Options::default() };
    let build = |tag, a_llc, b_llc, options: &Options, shared, all_linkers| Build {
        tag,
        a_llc,
        b_llc,
        options: options.clone(),
        shared,
        all_linkers,
    };
    let plain = Options::default();
    run_cases(
        "a64-call-random",
        seed,
        count,
        &[
            // The generator's own check: gcc and llc agree.
            build("LL", true, true, &plain, false, false),
            build("LH", true, false, &plain, false, false),
            build("HL", false, true, &plain, false, false),
            build("HH", false, false, &plain, false, true),
            build("HH-stress", false, false, &stress, false, false),
            // B a shared object: every call into it and back through the
            // PLT, every function address through the GOT.
            build("HH-pic", false, false, &pic, true, true),
            build("LH-pic", true, false, &pic, true, false),
        ],
    );
}

/// The handwritten forms of `tests/data/call/hand.ll`: a narrow leaf of
/// a `Mem`-class result, `byval` both ways, `returns_twice`, `musttail`
/// (stack and register arguments exchanged, a variadic thunk, through a
/// pointer), the extensions of the call site's and the declaration's
/// attributes, `noreturn`.
#[test]
fn handwritten_calls_run() {
    let dir = Scratch::new("a64-call-hand");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let ll = data("hand.ll");
    let c = data("hand.c");
    let hod_driver = toolchain.compile_c(&dir, "driver-hod", &c, &["-DHOD"]);
    let llc_driver = toolchain.compile_c(&dir, "driver-llc", &c, &[]);
    let runs: [(&str, bool, Options); 3] = [
        ("hod", false, Options::default()),
        ("stress", false, Options { stress: hod_cg::Stress::ALL, ..Options::default() }),
        ("llc", true, Options::default()),
    ];
    for (tag, with_llc, options) in runs {
        let Some(obj) = object_with(&dir, &format!("hand-{tag}"), &ll, with_llc, &options) else {
            return;
        };
        let driver = if with_llc { &llc_driver } else { &hod_driver };
        link_run_all(
            &toolchain,
            &dir,
            &format!("hand-{tag}"),
            &[],
            &[driver, &obj],
            "done:",
            tag != "hod",
        );
    }
}

#[test]
fn the_remaining_forms_are_refused_by_name() {
    assert_refused(&[
        ("define i64 @f(ptr %ap) { %r = va_arg ptr %ap, i64\n ret i64 %r }", call::VA_ARG),
        (
            "declare void @llvm.va_start.p0(ptr)\ndefine void @f(ptr %a) { call void @llvm.va_start.p0(ptr %a)\n ret void }",
            call::VA_INTRINSIC,
        ),
        (
            "define void @f() { call void asm sideeffect \"nop\", \"\"()\n ret void }",
            call::CALL_INLINE_ASM,
        ),
        (
            "declare void @g()\ndefine void @f() { call void @g() [ \"deopt\"() ]\n ret void }",
            call::CALL_BUNDLE,
        ),
    ]);
}

#[test]
#[ignore = "debugging aid"]
fn dbg_dump() {
    let text = std::fs::read_to_string(std::env::var("DBG_LL").unwrap()).unwrap();
    let (module, bodies) = parse(&format!("{}{text}", ll_header(22)));
    let emitted = emit(&module, &bodies, &Options::default());
    std::fs::write(std::env::var("DBG_O").unwrap(), hod_obj::elf::to_vec(&emitted.object).unwrap())
        .unwrap();
}
