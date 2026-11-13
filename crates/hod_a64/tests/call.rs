//! The call group (WP-038, `spec/codegen.md` §5.5): calls, `invoke`,
//! `musttail`, landing pads, `resume`, variadic definitions and the
//! `va_*` intrinsics, run natively against gcc and `llc`.

#![allow(clippy::similar_names, clippy::too_many_lines, clippy::too_many_arguments)]

#[path = "common/callgen.rs"]
mod callgen;
mod common;

use std::fmt::Write as _;
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
fn object_with(
    dir: &Scratch,
    name: &str,
    text: &str,
    with_llc: bool,
    options: Options,
) -> Option<PathBuf> {
    if with_llc {
        let pic = options.reloc_model == hod_cg::RelocModel::Pic;
        let flags: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
        return llc(dir, name, &format!("{}{text}", ll_header(llc_major())), flags);
    }
    let (module, bodies) = parse(&format!("{}{text}", ll_header(22)));
    let emitted = emit(&module, &bodies, &options);
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
#[allow(clippy::struct_excessive_bools)]
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
        let Some(obj_a) = object_with(&dir, &format!("a-{tag}"), &a, build.a_llc, build.options)
        else {
            return;
        };
        let Some(obj_b) = object_with(&dir, &format!("b-{tag}"), &b, build.b_llc, build.options)
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
            assert!(
                exit_code(&ran) == Some(0) && out.contains("checked"),
                "{label} {tag} {}: {out}{}\n{}",
                linker.name,
                ran.stderr_text(),
                failing_cases(&out, &[&a, &b, &c])
            );
        }
    }
    // Calls and function addresses take the relocations `llc` gives them
    // (`hod_cg::call_access`, `sym_access`).
    // `llc` shares one address of a symbol between the arguments of a
    // block, so an address's relocations are compared as a set.
    let find = |tag: &str| objects_a.iter().find(|(t, _)| *t == tag).map(|(_, p)| p.clone());
    for (ours, theirs) in [("HH-pic", "LH-pic"), ("HH", "LL")] {
        let (Some(hod), Some(llc)) = (find(ours), find(theirs)) else { continue };
        let (hod, llc) = (call_relocs(&hod), call_relocs(&llc));
        let differ: Vec<_> = hod
            .keys()
            .chain(llc.keys())
            .filter(|key| {
                let call = key.0.ends_with("CALL26") || key.0.ends_with("JUMP26");
                let (a, b) = (hod.get(*key), llc.get(*key));
                if call { a != b } else { a.is_some() != b.is_some() }
            })
            .map(|key| (key, hod.get(key), llc.get(key)))
            .collect();
        assert!(differ.is_empty(), "{label} {ours}: relocations differ from llc's: {differ:?}");
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
    let count = var("HOD_CALL_COUNT").map_or(600, |n| n as usize);
    let pic = Options { reloc_model: hod_cg::RelocModel::Pic, ..Options::default() };
    let stress = Options { stress: hod_cg::Stress::ALL, ..Options::default() };
    let build = |tag, a_llc, b_llc, options: &Options, shared, all_linkers| Build {
        tag,
        a_llc,
        b_llc,
        options: *options,
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
        let Some(obj) = object_with(&dir, &format!("hand-{tag}"), &ll, with_llc, options) else {
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

/// Real exceptions through hod's frames (`tests/data/call/eh.ll`, driven
/// by the C++ program `eh.cpp` built with g++): cleanups and `resume`, a
/// catch by type, a rethrow from a handler, nested pads, nested try in
/// one frame, two clauses, a satisfied filter, a `noreturn` thrower, an
/// `invoke` with stack arguments (its pad stub), a dynamic alloca, a
/// throw from hod, a frame without a personality, and a function in a
/// COMDAT group defined in two objects (`eh_comdat.ll`). The tables are
/// `emit_module`'s own. Static and stressed under every linker, then
/// position-independent in a shared object under every linker; `llc`'s
/// objects of the same modules are the control, both ways.
#[test]
fn exceptions_unwind_through_hod_frames() {
    let dir = Scratch::new("a64-call-eh");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let Some(gxx) = hod_testutil::tool("g++") else { return };
    let source = dir.write("eh.cpp", data("eh.cpp"));
    let driver = dir.join("eh-driver.o");
    let mut command = std::process::Command::new(&gxx);
    command.args(["-c", "-O1", "-fPIC"]).arg(&source).arg("-o").arg(&driver);
    let built = hod_testutil::run_with(&mut command, &[], std::time::Duration::from_mins(2));
    assert!(built.success(), "g++: {}", built.stderr_text());
    let libstdcxx = hod_testutil::run(&gxx, ["-print-file-name=libstdc++.so"]).stdout_text();
    let libstdcxx = PathBuf::from(libstdcxx.trim());
    let (eh, eh_comdat) = (data("eh.ll"), data("eh_comdat.ll"));
    let pic = Options { reloc_model: hod_cg::RelocModel::Pic, ..Options::default() };
    let stress = Options { stress: hod_cg::Stress::ALL, ..Options::default() };
    for (tag, with_llc, options, shared) in [
        ("llc", true, Options::default(), false),
        ("hod", false, Options::default(), false),
        ("stress", false, stress, false),
        ("pic", false, pic, true),
        ("llc-pic", true, pic, true),
    ] {
        let Some(obj) = object_with(&dir, &format!("eh-{tag}"), &eh, with_llc, options) else {
            return;
        };
        let comdat = object_with(&dir, &format!("ehc-{tag}"), &eh_comdat, with_llc, options)
            .expect("the second object");
        // `h_comdat` and its exception table are in its COMDAT group.
        if let Some(readelf) = hod_testutil::tool("llvm-readelf") {
            let groups =
                hod_testutil::run(readelf, [std::ffi::OsStr::new("-gW"), comdat.as_os_str()])
                    .stdout_text();
            assert!(groups.contains("[h_comdat]"), "eh-{tag}: no group\n{groups}");
            if !with_llc {
                assert!(groups.contains("gcc_except_table"), "eh-{tag}: LSDA outside\n{groups}");
            }
        }
        if !shared {
            let inputs: [&std::path::Path; 4] = [&driver, &obj, &comdat, &libstdcxx];
            link_run_all(&toolchain, &dir, &format!("eh-{tag}"), &[], &inputs, "eh done", false);
            continue;
        }
        for linker in &toolchain.linkers {
            let lib = dir.join(format!("libeh-{tag}-{}.so", linker.name));
            toolchain.link_ok(linker, &["-shared"], &[&obj, &comdat], &lib);
            let rpath = format!("-Wl,-rpath,{}", dir.path().display());
            let exe = dir.join(format!("eh-{tag}-{}", linker.name));
            let ran = toolchain.link_and_run(
                linker,
                &["-rdynamic", &rpath],
                &[&driver, &lib, &libstdcxx],
                &exe,
            );
            let out = ran.stdout_text();
            assert!(
                exit_code(&ran) == Some(0) && out.contains("eh done"),
                "eh-{tag} {}: exit {:?}\n{out}{}",
                linker.name,
                exit_code(&ran),
                ran.stderr_text()
            );
        }
    }
}

/// A type of the oracle chains: what `hod_interp` holds.
#[derive(Clone, Copy, PartialEq, Debug)]
enum OT {
    Int(u32, &'static str),
    Float,
    Double,
}

impl OT {
    fn ir(self) -> String {
        match self {
            OT::Int(n, _) => format!("i{n}"),
            OT::Float => "float".into(),
            OT::Double => "double".into(),
        }
    }
    fn attr(self) -> &'static str {
        match self {
            OT::Int(_, ext) => ext,
            _ => "",
        }
    }
    fn same(self, other: OT) -> bool {
        self.ir() == other.ir()
    }
    fn c(self) -> &'static str {
        match self {
            OT::Int(n, _) if n <= 64 => "uint64_t",
            OT::Int(..) => "unsigned __int128",
            OT::Float => "float",
            OT::Double => "double",
        }
    }
}

fn oracle_type(rng: &mut Rng) -> OT {
    const WIDTHS: [u32; 12] = [1, 8, 13, 16, 31, 32, 33, 64, 65, 100, 127, 128];
    match rng.below(8) {
        0 => OT::Float,
        1 => OT::Double,
        _ => {
            let n = *rng.pick(&WIDTHS);
            let ext = *rng.pick(&["", " zeroext", " signext"]);
            OT::Int(n, ext)
        }
    }
}

fn oracle_value(rng: &mut Rng, t: OT) -> hod_interp::Val {
    use hod_interp::Val;
    match t {
        OT::Int(n, _) => {
            let mask = if n == 128 { u128::MAX } else { (1u128 << n) - 1 };
            Val::Int { bits: n, value: rng.u128() & mask }
        }
        OT::Float => Val::Float(f32::from_bits(rng.next() as u32 & 0x3fff_ffff).to_bits()),
        OT::Double => Val::Double(rng.next() & 0x3fff_ffff_ffff_ffff),
    }
}

fn oracle_ir_const(v: &hod_interp::Val) -> String {
    use hod_interp::Val;
    match *v {
        Val::Int { bits, value } => {
            let signed = if bits < 128 && value >> (bits - 1) & 1 == 1 {
                (value | !((1u128 << bits) - 1)) as i128
            } else {
                value as i128
            };
            format!("i{bits} {signed}")
        }
        Val::Float(b) => format!("float 0x{:016X}", f64::from(f32::from_bits(b)).to_bits()),
        Val::Double(b) => format!("double 0x{b:016X}"),
        _ => unreachable!("only scalars"),
    }
}

/// The result line the driver prints for case `k`.
fn oracle_line(k: usize, v: &hod_interp::Val) -> String {
    use hod_interp::Val;
    let bits = match *v {
        Val::Int { value, .. } => value,
        Val::Float(b) => u128::from(b),
        Val::Double(b) => u128::from(b),
        _ => unreachable!("only scalars"),
    };
    format!("r {k} {:016x}{:016x}", (bits >> 64) as u64, bits as u64)
}

/// `nest` (in `x15`) and `byref` (a plain pointer) between a module
/// compiled by hod and one compiled by `llc`, both ways.
#[test]
fn nest_and_byref_agree_with_llc() {
    let dir = Scratch::new("a64-call-nest");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let half = |me: &str, other: &str| {
        format!(
            "declare ptr @{other}_nest(ptr nest, i64)\n\
             declare ptr @{other}_byref(i64, ptr byref([5 x i64]) align 8)\n\
             define ptr @{me}_nest(ptr nest %p, i64 %x) {{\n  ret ptr %p\n}}\n\
             define ptr @{me}_byref(i64 %x, ptr byref([5 x i64]) align 8 %p) {{\n  ret ptr %p\n}}\n\
             define ptr @{me}_call_nest(ptr %p) {{\n  %r = call ptr @{other}_nest(ptr nest %p, i64 1)\n  ret ptr %r\n}}\n\
             define ptr @{me}_call_byref(ptr %p) {{\n  %r = call ptr @{other}_byref(i64 1, ptr byref([5 x i64]) align 8 %p)\n  ret ptr %r\n}}\n"
        )
    };
    let Some(theirs) = object_with(&dir, "llc-half", &half("l", "h"), true, Options::default())
    else {
        return;
    };
    let ours =
        object_with(&dir, "hod-half", &half("h", "l"), false, Options::default()).expect("hod");
    let c = "#include <stdio.h>\nlong g[5];\n\
             void *h_call_nest(void *), *l_call_nest(void *), *h_call_byref(void *), *l_call_byref(void *);\n\
             int main(void) {\n  int ok = h_call_nest(g) == g && l_call_nest(g) == g\n\
             && h_call_byref(g) == g && l_call_byref(g) == g;\n  printf(\"done\\n\");\n  return !ok;\n}\n";
    let driver = toolchain.compile_c(&dir, "driver", c, &[]);
    link_run_all(&toolchain, &dir, "nest", &[], &[&driver, &ours, &theirs], "done", true);
}

/// `hod_interp` is the oracle: chains `o_k` -> `p_k` -> `q_k` of IR
/// functions passing integers of every width (with the extension
/// attributes on the inner calls), `float` and `double` in registers and
/// on the stack, permuted, duplicated and mixed with constants; `q_k`
/// returns one of its parameters. The interpreter's results must be what
/// the native program, compiled by hod plainly and stressed, prints.
#[test]
fn chains_agree_with_the_interpreter() {
    use hod_interp::Val;
    let dir = Scratch::new("a64-call-oracle");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let mut rng = Rng(0x38_0e1e);
    let mut ir = String::new();
    let mut c = String::from("#include <stdio.h>\n#include <stdint.h>\n#include <string.h>\n");
    let mut main = String::from("int main(void) {\n");
    let mut entries = Vec::new();
    for k in 0..150 {
        // q_k: its parameters and the one it returns.
        let q: Vec<OT> = (0..=rng.below(16)).map(|_| oracle_type(&mut rng)).collect();
        let pick = rng.below(q.len() as u64) as usize;
        let ret = q[pick];
        // A caller's arguments for `callee`: its own parameters (`have`,
        // with their values) or constants. Returns the argument texts.
        let call_args =
            |rng: &mut Rng, callee: &[OT], values: &[Val], have: &mut Vec<(OT, Val)>| {
                let mut args = Vec::new();
                for (t, v) in callee.iter().zip(values) {
                    let fits: Vec<usize> = (0..have.len())
                        .filter(|&i| have[i].0.same(*t) && have[i].1 == *v)
                        .collect();
                    let text = if !fits.is_empty() && rng.chance(50) {
                        format!("{}{} %a{}", t.ir(), t.attr(), rng.pick(&fits))
                    } else if rng.chance(30) {
                        let text = oracle_ir_const(v);
                        let (ty, rest) = text.split_once(' ').expect("type and value");
                        format!("{ty}{} {rest}", t.attr())
                    } else {
                        let attr = if let OT::Int(..) = t { oracle_type_ext(rng) } else { "" };
                        have.push((
                            match t {
                                OT::Int(n, _) => OT::Int(*n, attr),
                                other => *other,
                            },
                            v.clone(),
                        ));
                        format!("{}{} %a{}", t.ir(), t.attr(), have.len() - 1)
                    };
                    args.push(text);
                }
                args
            };
        let q_values: Vec<Val> = q.iter().map(|t| oracle_value(&mut rng, *t)).collect();
        let mut p_have = Vec::new();
        let q_args = call_args(&mut rng, &q, &q_values, &mut p_have);
        let p_types: Vec<OT> = p_have.iter().map(|(t, _)| *t).collect();
        let p_values: Vec<Val> = p_have.iter().map(|(_, v)| v.clone()).collect();
        let mut o_have = Vec::new();
        let p_args = call_args(&mut rng, &p_types, &p_values, &mut o_have);
        // The driver passes plain values: no attributes on `o_k`.
        let o_types: Vec<OT> = o_have
            .iter()
            .map(|(t, _)| match t {
                OT::Int(n, _) => OT::Int(*n, ""),
                o => *o,
            })
            .collect();
        let list = |types: &[OT]| -> String {
            types
                .iter()
                .enumerate()
                .map(|(i, t)| format!("{}{} %a{i}", t.ir(), t.attr()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let rty = ret.ir();
        let rattr = ret.attr();
        write!(ir, "define{rattr} {rty} @q_{k}({}) {{\n  ret {rty} %a{pick}\n}}\n", list(&q))
            .unwrap();
        write!(
            ir,
            "define {rty} @p_{k}({}) {{\n  %r = call{rattr} {rty} @q_{k}({})\n  ret {rty} %r\n}}\n",
            list(&p_types),
            q_args.join(", ")
        )
        .unwrap();
        write!(
            ir,
            "define {rty} @o_{k}({}) {{\n  %r = call {rty} @p_{k}({})\n  ret {rty} %r\n}}\n",
            list(&o_types),
            p_args.join(", ")
        )
        .unwrap();
        // The driver.
        let protos: Vec<&str> = o_types.iter().map(|t| t.c()).collect();
        let protos = if protos.is_empty() { "void".to_owned() } else { protos.join(", ") };
        writeln!(c, "{} o_{k}({protos});", ret.c()).unwrap();
        let args: Vec<String> = o_have
            .iter()
            .map(|(_, v)| match *v {
                Val::Int { value, .. } => format!(
                    "((unsigned __int128)0x{:x}ULL << 64 | 0x{:x}ULL)",
                    (value >> 64) as u64,
                    value as u64
                ),
                Val::Float(b) => {
                    format!("({{ uint32_t b_ = 0x{b:x}; float f_; memcpy(&f_, &b_, 4); f_; }})")
                }
                Val::Double(b) => {
                    format!("({{ uint64_t b_ = 0x{b:x}ULL; double f_; memcpy(&f_, &b_, 8); f_; }})")
                }
                _ => unreachable!(),
            })
            .collect();
        let read = match ret {
            OT::Int(n, _) => {
                let mask = if n == 128 { u128::MAX } else { (1u128 << n) - 1 };
                format!(
                    "unsigned __int128 v = (unsigned __int128)o_{k}({}) & ((unsigned __int128)0x{:x}ULL << 64 | 0x{:x}ULL);",
                    args.join(", "),
                    (mask >> 64) as u64,
                    mask as u64
                )
            }
            OT::Float => format!(
                "float f = o_{k}({}); uint32_t b; memcpy(&b, &f, 4); unsigned __int128 v = b;",
                args.join(", ")
            ),
            OT::Double => format!(
                "double f = o_{k}({}); uint64_t b; memcpy(&b, &f, 8); unsigned __int128 v = b;",
                args.join(", ")
            ),
        };
        writeln!(main,
            "  {{ {read} printf(\"r {k} %016llx%016llx\\n\", (unsigned long long)(v >> 64), (unsigned long long)v); }}"
        ).unwrap();
        entries.push((k, o_have.into_iter().map(|(_, v)| v).collect::<Vec<Val>>()));
    }
    main.push_str("  return 0;\n}\n");
    c.push_str(&main);
    let text = format!("{}{ir}", ll_header(22));
    let (module, bodies) = parse(&text);
    let program = hod_interp::Program::with_bodies(&module, &bodies).expect("a program");
    let mut expected = String::new();
    for (k, args) in &entries {
        let entry = module.find_symbol(format!("o_{k}").as_bytes()).expect("o_k");
        let outcome = hod_interp::Interp::new(&program).run(entry, args);
        let result = outcome.result.unwrap_or_else(|e| panic!("o_{k}: {e:?}")).expect("a value");
        expected.push_str(&oracle_line(*k, &result));
        expected.push('\n');
    }
    let driver = toolchain.compile_c(&dir, "driver", &c, &["-w"]);
    for (tag, stress) in [("plain", hod_cg::Stress::default()), ("stress", hod_cg::Stress::ALL)] {
        let emitted = emit(&module, &bodies, &Options { stress, ..Options::default() });
        let object = write_object(&dir, &format!("oracle-{tag}.o"), &emitted);
        let exe = dir.join(format!("oracle-{tag}"));
        let linker = &toolchain.linkers[0];
        let ran = toolchain.link_and_run(linker, &[], &[&driver, &object], &exe);
        assert_eq!(exit_code(&ran), Some(0), "{tag}");
        let got = ran.stdout_text();
        for (want, have) in expected.lines().zip(got.lines()) {
            assert_eq!(have, want, "{tag}: the native run and the interpreter disagree\n{text}");
        }
        assert_eq!(got.lines().count(), entries.len(), "{tag}");
    }
}

/// An extension attribute a caller adds to an integer argument.
fn oracle_type_ext(rng: &mut Rng) -> &'static str {
    const EXTS: [&str; 3] = ["", " zeroext", " signext"];
    EXTS[rng.below(3) as usize]
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
            // WP-103: a template is assembled; one hod_asm does not read
            // is refused by its name.
            "define void @f() { call void asm sideeffect \"frob\", \"\"()\n ret void }",
            "asm:mnemonic/frob",
        ),
        (
            "declare void @g()\ndefine void @f() { call void @g() [ \"deopt\"() ]\n ret void }",
            call::CALL_BUNDLE,
        ),
        (
            "declare void @g(ptr sret([5 x i64]))\n\
             define void @f(ptr %p) { call void @g(ptr byval([5 x i64]) %p)\n ret void }",
            hod_ir::AttrConflict::NAME,
        ),
        (
            "declare i32 @p(...)\ndeclare void @g()\n\
             define void @f() personality ptr @p { invoke void @g() to label %a unwind label %b\n\
             a:\n ret void\nb:\n %l = landingpad { ptr, i64 } cleanup\n ret void }",
            call::LANDING_PAD,
        ),
    ]);
    // An extended `i7` in a register or on the stack, an extended `i100`,
    // and a matching direct call that takes the declaration's attributes,
    // are lowered.
    for lowered in [
        "declare void @g(i7 zeroext)\ndefine void @f(i7 %x) { call void @g(i7 zeroext %x)\n ret void }",
        "declare void @g(i64, i64, i64, i64, i64, i64, i64, i64, i7 zeroext)\n\
         define void @f(i7 %x) { call void @g(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i7 zeroext %x)\n ret void }",
        "declare void @g(i100)\ndefine void @f(i100 %x) { call void @g(i100 signext %x)\n ret void }",
        "declare void @g(i100 zeroext)\ndefine void @f(i100 %x) { call void @g(i100 %x)\n ret void }",
        "declare void @g(ptr byval([5 x i64]) align 8)\ndefine void @f(ptr %p) { call void @g(ptr %p)\n ret void }",
    ] {
        assert_eq!(common::compile_ll(lowered), Ok(()), "{lowered}");
    }
}

/// A return value wider than 64 bits with `signext` is owed extended to
/// 128 bits (`abi-a64.md` §7): its last piece is extended.
#[test]
fn a_wide_signext_return_is_extended() {
    let dir = Scratch::new("a64-call-wide-ret");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let text = "define signext i65 @f(i65 %x) {\n  ret i65 %x\n}\n";
    let obj = object_with(&dir, "wide", text, false, Options::default()).expect("hod");
    let c = "#include <stdio.h>\n__int128 f(__int128);\nint main(void) {\n\
             __int128 r = f(((__int128)1 << 64) | 5);\n\
             int ok = (unsigned long long)(r >> 64) == ~0ULL;\n\
             printf(\"done\\n\");\n  return !ok;\n}\n";
    let driver = toolchain.compile_c(&dir, "driver", c, &[]);
    link_run_all(&toolchain, &dir, "wide", &[], &[&driver, &obj], "done", true);
}

/// A direct call through a function type other than the callee's takes
/// none of the declaration's attributes (`spec/codegen.md` §7.1, as
/// `llc` lowers it): lowered, not refused.
#[test]
fn a_mismatched_direct_call_takes_no_declared_attributes() {
    let functions = "declare void @g(ptr byval([5 x i64]) align 8)\n\
                     define void @f(ptr %p) { call void (ptr, i64) @g(ptr %p, i64 1)\n ret void }";
    assert_eq!(common::compile_ll(functions), Ok(()));
}

/// `spec/codegen.md` §7.1, effective attributes, run: `tests/data/call/
/// attrs.ll` calls gcc's functions and `attrs_callee.ll`'s through
/// declarations whose attributes the calls must not take (another
/// function type: `byval`, `sret`, `nest`, variadic against fixed both
/// ways, a result) and must take (the callee's own type: `byval` beside
/// the site's `align`, `signext`). The caller and the callee module by
/// hod and by `llc` in every pairing, hod also stressed; every linker
/// where hod is involved.
#[test]
fn declared_attributes_follow_the_function_type() {
    let dir = Scratch::new("a64-call-attrs");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let caller = data("attrs.ll");
    let callee = data("attrs_callee.ll");
    let driver = toolchain.compile_c(&dir, "attrs-driver", &data("attrs.c"), &[]);
    let stress = Options { stress: hod_cg::Stress::ALL, ..Options::default() };
    let plain = Options::default();
    for (tag, caller_llc, callee_llc, options) in [
        ("LL", true, true, plain),
        ("HL", false, true, plain),
        ("LH", true, false, plain),
        ("HH", false, false, plain),
        ("HH-stress", false, false, stress),
    ] {
        let Some(a) = object_with(&dir, &format!("attrs-{tag}"), &caller, caller_llc, options)
        else {
            return;
        };
        let Some(b) =
            object_with(&dir, &format!("attrs-callee-{tag}"), &callee, callee_llc, options)
        else {
            return;
        };
        let name = format!("attrs-{tag}");
        link_run_all(&toolchain, &dir, &name, &[], &[&driver, &a, &b], "0 failed", tag == "LL");
    }
}

/// `abi-a64.md` §7, run: integers with `zeroext` or `signext` of odd
/// widths and of widths over 64 bits that are not multiples of 64, in
/// registers and on the stack, as arguments and results
/// (`tests/data/call/ext.ll`). Every value starts as junk above its
/// width; gcc's checkers see every bit of the extended carrier. The
/// caller and the callee module by hod and by `llc` in every pairing,
/// hod also stressed; every linker where hod is involved.
#[test]
fn extended_integers_of_every_width_run() {
    let dir = Scratch::new("a64-call-ext");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let caller = data("ext.ll");
    let callee = data("ext_callee.ll");
    let driver = toolchain.compile_c(&dir, "ext-driver", &data("ext.c"), &[]);
    let stress = Options { stress: hod_cg::Stress::ALL, ..Options::default() };
    let plain = Options::default();
    for (tag, caller_llc, callee_llc, options) in [
        ("LL", true, true, plain),
        ("HL", false, true, plain),
        ("LH", true, false, plain),
        ("HH", false, false, plain),
        ("HH-stress", false, false, stress),
    ] {
        let Some(a) = object_with(&dir, &format!("ext-{tag}"), &caller, caller_llc, options) else {
            return;
        };
        let Some(b) = object_with(&dir, &format!("ext-callee-{tag}"), &callee, callee_llc, options)
        else {
            return;
        };
        let name = format!("ext-{tag}");
        link_run_all(&toolchain, &dir, &name, &[], &[&driver, &a, &b], " 0 failed", tag == "LL");
    }
}

/// `spec/codegen.md` §2.2 and §4.8, run: a `Mem`-class argument whose
/// integer leaves have odd widths (`{ i17, i64 }`, `i65`, `[2 x i9]`, a
/// nest of them), in registers and on the stack, arrives with junk above
/// every leaf (`abi-a64.md` §7 promises nothing there), and its home
/// must hold zeros above each width (`tests/data/call/odd.ll`). The home
/// is observed directly: the callee stores the argument to a buffer of
/// the driver, which checks every leaf's bytes up to its store size
/// (`llc` zero-masks there too, so this holds in every pairing); and
/// through what the callee passes on to gcc's checkers, which, for hod's
/// callee, also check those zeros. gcc's driver calls the callee
/// directly and through a forwarding caller; the caller and the callee
/// module by hod and by `llc` in every pairing, hod also stressed; every
/// linker where hod is involved.
#[test]
fn odd_leaves_of_incoming_arguments_are_cleared() {
    let dir = Scratch::new("a64-call-odd");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let caller = data("odd.ll");
    let callee = data("odd_callee.ll");
    let c = data("odd.c");
    let hod_driver = toolchain.compile_c(&dir, "odd-driver-hod", &c, &["-DHOD"]);
    let llc_driver = toolchain.compile_c(&dir, "odd-driver-llc", &c, &[]);
    let stress = Options { stress: hod_cg::Stress::ALL, ..Options::default() };
    let plain = Options::default();
    for (tag, caller_llc, callee_llc, options) in [
        ("LL", true, true, plain),
        ("HL", false, true, plain),
        ("LH", true, false, plain),
        ("HH", false, false, plain),
        ("HH-stress", false, false, stress),
    ] {
        let Some(a) = object_with(&dir, &format!("odd-{tag}"), &caller, caller_llc, options) else {
            return;
        };
        let Some(b) = object_with(&dir, &format!("odd-callee-{tag}"), &callee, callee_llc, options)
        else {
            return;
        };
        let driver = if callee_llc { &llc_driver } else { &hod_driver };
        let name = format!("odd-{tag}");
        link_run_all(&toolchain, &dir, &name, &[], &[driver, &a, &b], " 0 failed", tag == "LL");
    }
}

/// `attr:alignstack`, run (`tests/data/call/alignstack.ll`): on block
/// arguments that go to the stack (`abi-a64.md` §3.3: `[2 x double]` and
/// `[3 x float]` aligned past their own alignment, `[2 x fp128]`,
/// `[2 x float] alignstack(8)`, the corpus's forms), and on a function
/// (`alignstack(32)` with a 32-aligned slot, `alignstack(16)`). gcc's
/// driver calls the callee directly and through a forwarding caller; the
/// caller and the callee module by hod and by `llc` in every pairing, hod
/// also stressed; every linker where hod is involved.
#[test]
fn alignstack_arguments_and_functions_run() {
    let dir = Scratch::new("a64-call-alignstack");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let caller = data("alignstack.ll");
    let callee = data("alignstack_callee.ll");
    let driver = toolchain.compile_c(&dir, "alignstack-driver", &data("alignstack.c"), &[]);
    let stress = Options { stress: hod_cg::Stress::ALL, ..Options::default() };
    let plain = Options::default();
    for (tag, caller_llc, callee_llc, options) in [
        ("LL", true, true, plain),
        ("HL", false, true, plain),
        ("LH", true, false, plain),
        ("HH", false, false, plain),
        ("HH-stress", false, false, stress),
    ] {
        let Some(a) = object_with(&dir, &format!("as-{tag}"), &caller, caller_llc, options) else {
            return;
        };
        let Some(b) = object_with(&dir, &format!("as-callee-{tag}"), &callee, callee_llc, options)
        else {
            return;
        };
        let name = format!("alignstack-{tag}");
        link_run_all(&toolchain, &dir, &name, &[], &[&driver, &a, &b], " 0 failed", tag == "LL");
    }
}

/// `spec/codegen.md` §7.1, "Tail kinds": `hod_cg::abi::TargetStackArgs`
/// is the `StackArgs` hook `classify` decides `musttail` calls with
/// (`spec/ir.md` §8.6). With it, the handwritten `musttail` forms that
/// `handwritten_calls_run` runs (stack and register arguments exchanged,
/// variadic thunks forwarding `...`, an indirect call) meet no
/// `musttail:` rule; a call that needs a larger stack-argument area than
/// its caller's has is `musttail:stack-args-grow`, also by variable
/// arguments it lists before a forwarded `...` (WP-115), and every
/// `musttail` call without the hook is `musttail:stack-args-unknown`. The areas are those of the
/// effective attributes: a `byval` the call takes from the declaration
/// counts.
#[test]
fn the_stack_args_hook_decides_musttail() {
    use hod_ir::{Arch, Census, Classifier, Support};
    let hook = hod_cg::abi::TargetStackArgs::new(&hod_a64::A64);
    let shipped = Support::shipped(Arch::Aarch64);
    let musttail_rules = |text: &str, with_hook: bool| -> Vec<String> {
        let (module, bodies) = parse(&format!("{}{text}", ll_header(22)));
        let classifier = Classifier::new(shipped);
        let classifier = if with_hook { classifier.with_stack_args(&hook) } else { classifier };
        let mut census = Census::new();
        for body in &bodies {
            classifier.census_body(&module, body, &mut census);
        }
        let mut found: Vec<String> = census
            .entries()
            .iter()
            .map(|entry| entry.construct.clone())
            .filter(|name| name.starts_with("musttail:") || name == "tail:musttail")
            .collect();
        found.dedup();
        found
    };
    let hand = data("hand.ll");
    let hand = hand.lines().filter(|line| !line.starts_with("target ")).collect::<Vec<_>>();
    let hand = hand.join("\n");
    assert_eq!(musttail_rules(&hand, true), Vec::<String>::new());
    assert_eq!(musttail_rules(&hand, false), ["musttail:stack-args-unknown"]);

    let nine = "i64, i64, i64, i64, i64, i64, i64, i64, i64";
    let cases: [(String, &[&str]); 5] = [
        (
            format!(
                "declare i64 @g({nine})\n\
                 define i64 @f({nine}) {{\n  %r = musttail call i64 @g(i64 0, i64 0, i64 0, i64 0, \
                 i64 0, i64 0, i64 0, i64 0, i64 1)\n  ret i64 %r\n}}\n"
            ),
            &[],
        ),
        (
            format!(
                "declare i64 @g({nine})\n\
                 define i64 @f(i64 %x) {{\n  %r = musttail call i64 @g(i64 0, i64 0, i64 0, i64 0, \
                 i64 0, i64 0, i64 0, i64 0, i64 %x)\n  ret i64 %r\n}}\n"
            ),
            &["musttail:stack-args-grow"],
        ),
        (
            "declare void @g(ptr byval([4 x i64]) align 8)\n\
             define void @f(ptr %p) {\n  musttail call void @g(ptr %p)\n  ret void\n}\n"
                .into(),
            &["musttail:stack-args-grow"],
        ),
        // WP-115: variable arguments before a forwarded `...` are typed
        // from the body; in registers they fit, on the stack they grow.
        (
            "declare void @g(i32, ...)\n\
             define void @f(i32 %n, ...) {\n  musttail call void (i32, ...) @g(i32 %n, i64 1, ...)\n  \
             ret void\n}\n"
                .into(),
            &[],
        ),
        (
            "declare void @g(i32, ...)\n\
             define void @f(i32 %n, ...) {\n  musttail call void (i32, ...) @g(i32 %n, i64 1, i64 2, \
             i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, ...)\n  ret void\n}\n"
                .into(),
            &["musttail:stack-args-grow"],
        ),
    ];
    for (text, want) in &cases {
        assert_eq!(musttail_rules(text, true), *want, "{text}");
    }
}

/// `spec/codegen.md` §7.1, effective attributes, as a property:
/// `classify` and the call sequence see the same attributes for random
/// combinations of a declaration's and a call site's attributes, with
/// the call through the callee's own function type or another one
/// (an extra argument, variadic against fixed both ways). Both are
/// observed by what they refuse: `inreg` and `swiftself` are refused by
/// the shipped matrix (`attr:inreg`, `attr:swiftself`) and by `assign`
/// (`abi:inreg`, `abi:swiftself`), and an `sret` on one side with a
/// `byval` on the other is `call:attribute-conflict`. The expectation is
/// a model of `llc`'s rule written here, not `hod_ir`'s helper.
#[test]
fn classify_and_the_call_sequence_see_the_same_attributes() {
    const P0: [&str; 6] =
        ["", "inreg", "swiftself", "sret([2 x i64])", "byval([2 x i64])", "noundef"];
    const P1: [&str; 5] = ["", "signext", "zeroext", "inreg", "noundef"];
    const P2: [&str; 3] = ["", "inreg", "byval([2 x i64])"];
    const RET: [&str; 4] = ["", "signext", "zeroext", "inreg"];
    let mut rng = Rng(0x69_0001);
    let mut seen = [0usize; 4];
    for case in 0..600 {
        let pick = |rng: &mut Rng, pool: &[&'static str]| *rng.pick(pool);
        let mut decl =
            [pick(&mut rng, &RET), pick(&mut rng, &P0), pick(&mut rng, &P1), pick(&mut rng, &P2)];
        let mut site =
            [pick(&mut rng, &RET), pick(&mut rng, &P0), pick(&mut rng, &P1), pick(&mut rng, &P2)];
        // `sret` goes with a `void` result.
        let sret = decl[1].starts_with("sret") || site[1].starts_with("sret");
        let ret = if sret { "void" } else { "i8" };
        if sret {
            (decl[0], site[0]) = ("", "");
        }
        // 0: the callee's type; 1: an extra i64; 2: a variadic call type
        // of a fixed callee; 3: a variadic callee through its own type,
        // with a variable i64; 4: a variadic callee through a fixed type.
        let shape = rng.below(5) as usize;
        let decl_variadic = shape >= 3;
        let (call_ty, extra) = match shape {
            1 => ("(ptr, i8, ptr, i64)", ", i64 1"),
            2 => ("(ptr, i8, ptr, ...)", ""),
            3 => ("(ptr, i8, ptr, ...)", ", i64 1"),
            _ => ("(ptr, i8, ptr)", ""),
        };
        let matched = shape == 0 || shape == 3;
        let dots = if decl_variadic { ", ..." } else { "" };
        let text = format!(
            "declare {} {ret} @g(ptr {} , i8 {} , ptr {} {dots})\n\
             define void @f(ptr %p, i8 %x) {{\n  call {} {ret} {call_ty} @g(ptr {} %p, i8 {} %x, ptr {} %p{extra})\n  ret void\n}}\n",
            decl[0], decl[1], decl[2], decl[3], site[0], site[1], site[2], site[3]
        );
        // The model: the call site's attributes, and the declaration's
        // when the types match.
        let effective: Vec<&str> =
            site.iter().chain(if matched { &decl[..] } else { &[][..] }).copied().collect();
        let has = |what: &str| effective.contains(&what);
        let mem = |a: &str| {
            if a.starts_with("sret") {
                1
            } else if a.starts_with("byval") {
                2
            } else {
                0
            }
        };
        let conflict = matched && mem(site[1]) * mem(decl[1]) == 2;
        let refused: Vec<&str> = ["inreg", "swiftself"].into_iter().filter(|a| has(a)).collect();

        let (module, bodies) = parse(&format!("{}{text}", ll_header(22)));
        let census = hod_ir::census(&module, &bodies);
        for what in ["inreg", "swiftself"] {
            let row = format!("attr:{what}");
            let found = census.entries().iter().any(|entry| entry.construct == row);
            assert_eq!(found, refused.contains(&what), "case {case}: classify and {row}\n{text}");
        }
        let compiled = common::compile_ll(&text);
        let allowed: Vec<String> = refused
            .iter()
            .map(|a| format!("abi:{a}"))
            .chain(conflict.then(|| hod_ir::AttrConflict::NAME.to_owned()))
            .collect();
        match &compiled {
            Ok(()) => {
                assert!(allowed.is_empty(), "case {case}: compiled, expected {allowed:?}\n{text}");
            }
            Err(hod_cg::CgError::Unsupported { construct }) => assert!(
                allowed.iter().any(|name| name == construct),
                "case {case}: refused {construct}, expected {allowed:?}\n{text}"
            ),
            Err(other) => panic!("case {case}: {other}\n{text}"),
        }
        seen[usize::from(matched) * 2 + usize::from(compiled.is_ok())] += 1;
    }
    // Every quadrant of (matched, compiled) is exercised.
    assert!(seen.iter().all(|&n| n > 10), "{seen:?}");
}
