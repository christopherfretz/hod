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
    if with_llc {
        return llc(dir, name, &format!("{}{text}", ll_header(llc_major())), &[]);
    }
    let (module, bodies) = parse(&format!("{}{text}", ll_header(22)));
    let emitted = emit(&module, &bodies, &Options::default());
    Some(write_object(dir, &format!("{name}.o"), &emitted))
}

/// Generates `count` cases from `seed`, and runs them with modules A and
/// B compiled by every pair of hod and `llc` (all linkers for hod/hod).
fn run_cases(label: &str, seed: u64, count: usize) {
    let dir = Scratch::new(label);
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let mut rng = Rng(seed);
    let cases: Vec<Case> = (0..count).map(|k| callgen::random_case(&mut rng, k)).collect();
    let a = callgen::module_a(&cases, "");
    let b = callgen::module_b(&cases, "");
    let (module_b, _) = parse(&format!("{}{b}", ll_header(22)));
    let at = places(&module_b, &cases);
    let c = callgen::driver(&cases, &|k| at[k].clone());
    let driver = toolchain.compile_c(&dir, "driver", &c, &["-Wno-psabi", "-w"]);
    for (a_llc, b_llc) in [(true, true), (true, false), (false, true), (false, false)] {
        let tag = format!("{}{}", if a_llc { "L" } else { "H" }, if b_llc { "L" } else { "H" });
        let Some(obj_a) = object(&dir, &format!("a-{tag}"), &a, a_llc) else { return };
        let Some(obj_b) = object(&dir, &format!("b-{tag}"), &b, b_llc) else { return };
        let linkers = if tag == "HH" { &toolchain.linkers[..] } else { &toolchain.linkers[..1] };
        for linker in linkers {
            let exe = dir.join(format!("p-{tag}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, &obj_a, &obj_b], &exe);
            if exit_code(&ran) != Some(0) {
                let out = ran.stdout_text();
                panic!(
                    "{label} {tag} {}: {out}{}\n{}",
                    linker.name,
                    ran.stderr_text(),
                    failing_cases(&out, &[&a, &b, &c])
                );
            }
        }
    }
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
    run_cases("a64-call-random", seed, count);
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
