//! The call group (WP-038, `spec/codegen.md` §5.5): calls, `invoke`,
//! `musttail`, landing pads, `resume`, variadic definitions and the
//! `va_*` intrinsics, run natively against gcc and `llc`.

#[path = "common/callgen.rs"]
mod callgen;
mod common;
#[path = "common/unwind.rs"]
mod unwind;

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

/// Real exceptions through hod's frames (`tests/data/call/eh.ll`, driven
/// by the C++ program `eh.cpp` built with g++): cleanups and `resume`, a
/// catch by type, a rethrow from a handler, nested pads, two clauses, a
/// satisfied filter, a `noreturn` thrower, an `invoke` with stack
/// arguments (its pad stub), a frame without a personality. The tables
/// are written by the glue of `common/unwind.rs` until WP-049; `llc`'s
/// object of the same module is the control.
#[test]
fn exceptions_unwind_through_hod_frames() {
    let dir = Scratch::new("a64-call-eh");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let Some(gxx) = hod_testutil::tool("g++") else { return };
    let source = dir.write("eh.cpp", data("eh.cpp"));
    let driver = dir.join("eh-driver.o");
    let mut command = std::process::Command::new(&gxx);
    command.args(["-c", "-O1"]).arg(&source).arg("-o").arg(&driver);
    let built = hod_testutil::run_with(&mut command, &[], std::time::Duration::from_mins(2));
    assert!(built.success(), "g++: {}", built.stderr_text());
    let libstdcxx = hod_testutil::run(&gxx, ["-print-file-name=libstdc++.so"]).stdout_text();
    let libstdcxx = PathBuf::from(libstdcxx.trim());
    let ll = data("eh.ll");
    let (module, bodies) = parse(&format!("{}{ll}", ll_header(22)));
    for (tag, options) in [
        ("hod", Options::default()),
        ("stress", Options { stress: hod_cg::Stress::ALL, ..Options::default() }),
    ] {
        let mut emitted = emit(&module, &bodies, &options);
        unwind::add_eh_tables(&module, &mut emitted);
        let obj = write_object(&dir, &format!("eh-{tag}.o"), &emitted);
        let first_only = tag != "hod";
        link_run_all(
            &toolchain,
            &dir,
            &format!("eh-{tag}"),
            &[],
            &[&driver, &obj, &libstdcxx],
            "eh done",
            first_only,
        );
    }
    if let Some(obj) = llc(&dir, "eh-llc", &format!("{}{ll}", ll_header(llc_major())), &[]) {
        link_run_all(
            &toolchain,
            &dir,
            "eh-llc",
            &[],
            &[&driver, &obj, &libstdcxx],
            "eh done",
            true,
        );
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
            // `a64:call-wide-int-extension`, `a64:call-narrow-ext-stack-arg`.
            let ext = if matches!(n, 1 | 8 | 16 | 32) {
                *rng.pick(&["", " zeroext", " signext"])
            } else {
                ""
            };
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
        let q: Vec<OT> = (0..1 + rng.below(16)).map(|_| oracle_type(&mut rng)).collect();
        let pick = rng.below(q.len() as u64) as usize;
        let ret = q[pick];
        // A caller's arguments for `callee`: its own parameters (`have`,
        // with their values) or constants. Returns the argument texts.
        let call_args = |rng: &mut Rng,
                         callee: &[OT],
                         values: &[Val],
                         have: &mut Vec<(OT, Val)>| {
            let mut args = Vec::new();
            for (t, v) in callee.iter().zip(values) {
                let fits: Vec<usize> =
                    (0..have.len()).filter(|&i| have[i].0.same(*t) && have[i].1 == *v).collect();
                let text = if !fits.is_empty() && rng.chance(50) {
                    format!("{}{} %a{}", t.ir(), t.attr(), rng.pick(&fits))
                } else if rng.chance(30) {
                    let text = oracle_ir_const(v);
                    let (ty, rest) = text.split_once(' ').expect("type and value");
                    format!("{ty}{} {rest}", t.attr())
                } else {
                    let attr = if let OT::Int(n, _) = t { oracle_type_ext(rng, *n) } else { "" };
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
        ir.push_str(&format!(
            "define{rattr} {rty} @q_{k}({}) {{\n  ret {rty} %a{pick}\n}}\n",
            list(&q)
        ));
        ir.push_str(&format!(
            "define {rty} @p_{k}({}) {{\n  %r = call{rattr} {rty} @q_{k}({})\n  ret {rty} %r\n}}\n",
            list(&p_types),
            q_args.join(", ")
        ));
        ir.push_str(&format!(
            "define {rty} @o_{k}({}) {{\n  %r = call {rty} @p_{k}({})\n  ret {rty} %r\n}}\n",
            list(&o_types),
            p_args.join(", ")
        ));
        // The driver.
        let protos: Vec<&str> = o_types.iter().map(|t| t.c()).collect();
        let protos = if protos.is_empty() { "void".to_owned() } else { protos.join(", ") };
        c.push_str(&format!("{} o_{k}({protos});\n", ret.c()));
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
        main.push_str(&format!(
            "  {{ {read} printf(\"r {k} %016llx%016llx\\n\", (unsigned long long)(v >> 64), (unsigned long long)v); }}\n"
        ));
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

/// An extension attribute for an argument of `n` bits a caller adds.
fn oracle_type_ext(rng: &mut Rng, n: u32) -> &'static str {
    if matches!(n, 1 | 8 | 16 | 32) { *rng.pick(&["", " zeroext", " signext"]) } else { "" }
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
