//! The x86-64 call group (WP-125, `spec/codegen.md` §5.6.1,
//! `spec/abi-x64.md`): calls with `sret`, `byval`, stack arguments and
//! mixed classes, results in `rax:rdx`, `xmm0:xmm1` and in memory, the
//! extensions a caller and a callee owe, odd leaves, variadic calls and
//! definitions, `invoke`, landing pads and `resume` with real C++
//! exceptions, `musttail`, indirect calls. Every program is linked by GNU
//! ld, lld and mold and run (under `qemu-x86_64` on another host);
//! `llc`'s objects of the same modules are the control.

#![allow(clippy::similar_names, clippy::many_single_char_names, clippy::too_many_lines)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

// The signature machinery of `tests/abi` (WP-118), shared by path: the
// types and values of a signature, `llc`'s callers and callees, the
// driver and the comparison. Its random generator is in `grid.rs` beside
// that binary's tests, so `chain_cases` below draws its own.
#[path = "abi/emit.rs"]
#[allow(dead_code, reason = "shared with tests/abi")]
mod emit;
#[path = "abi/ir.rs"]
#[allow(dead_code, reason = "shared with tests/abi")]
mod ir;
#[path = "abi/run.rs"]
#[allow(dead_code, reason = "shared with tests/abi")]
mod run;
#[path = "abi/sigs.rs"]
#[allow(dead_code, reason = "shared with tests/abi")]
mod sigs;
#[path = "abi/ty.rs"]
#[allow(dead_code, reason = "shared with tests/abi")]
mod ty;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{CgError, Options, RelocModel, Stress};
use hod_ir::{Body, Module};
use hod_testutil::{Scratch, run_with, tool};
use hod_x64::call;
use linking::{Toolchain, exit_code};

/// The module header: the x86-64 data layout and triple.
fn header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// A module and its bodies from `.ll` text (the header added).
fn parse(text: &str) -> (Module, Vec<Body>) {
    let text = format!("{}{text}", header());
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, parsed| {
        let body =
            parsed.body.unwrap_or_else(|| panic!("hod_ll: unsupported {:?}", parsed.unsupported));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    (module, bodies)
}

fn pic() -> Options {
    Options { reloc_model: RelocModel::Pic, ..Options::default() }
}

/// The object of `text` by hod, or by `llc -O0 -fast-isel=0` when
/// `with_llc` (`None` when `llc` is missing).
fn object_with(
    dir: &Scratch,
    name: &str,
    text: &str,
    with_llc: bool,
    options: Options,
) -> Option<PathBuf> {
    if with_llc {
        let llc = tool("llc")?;
        let input = dir.write(&format!("{name}.ll"), format!("{}{text}", header()));
        let output = dir.join(format!("{name}.o"));
        let model = if options.reloc_model == RelocModel::Pic { "pic" } else { "static" };
        let mut command = Command::new(llc);
        command
            .args(["-O0", "-fast-isel=0", "-filetype=obj"])
            .arg(format!("-relocation-model={model}"))
            .arg(&input)
            .arg("-o")
            .arg(&output);
        let outcome = run_with(&mut command, &[], Duration::from_mins(2));
        assert!(outcome.success(), "llc {name}: {}", outcome.stderr_text());
        return Some(output);
    }
    let (module, bodies) = parse(text);
    let bytes = hod_x64::compile_to_object(&module, &bodies, &options)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    Some(dir.write(&format!("{name}.o"), bytes))
}

/// What compiling `text` reports: the first refusal, or `Ok`.
fn compile(text: &str) -> Result<(), CgError> {
    let (module, bodies) = parse(text);
    match hod_x64::compile_to_object(&module, &bodies, &pic()) {
        Ok(_) => Ok(()),
        Err(hod_x64::ModuleError::Function { error, .. }) => Err(error),
        Err(other) => panic!("{other}"),
    }
}

/// A file of `tests/data/call`.
fn data(name: &str) -> String {
    let path = format!("{}/tests/data/call/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Links `inputs` with every linker (the first only when `first_only`),
/// runs the program and requires exit 0 and `marker` in its output.
fn link_run_all(
    toolchain: &Toolchain,
    dir: &Scratch,
    name: &str,
    flags: &[&str],
    inputs: &[&Path],
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

/// The builds of a handwritten module: hod position-independent (a PIE,
/// every linker), hod static (`-no-pie`), hod stressed, and `llc`.
fn builds() -> [(&'static str, bool, Options, &'static str); 4] {
    let stress = Options { stress: Stress::ALL, ..pic() };
    [
        ("hod", false, pic(), "-pie"),
        ("static", false, Options::default(), "-no-pie"),
        ("stress", false, stress, "-pie"),
        ("llc", true, pic(), "-pie"),
    ]
}

/// `tests/data/call/hand.ll` driven by `hand.c`: `sret` (the pointer
/// back in `rax`, also from a variadic definition and through
/// `musttail`), `byval` both ways, stack arguments, mixed classes,
/// results in `rax:rdx`, `xmm0:xmm1`, `rax:xmm0` and in memory, the
/// extensions of §7 for every width class both ways, odd leaves of a
/// `Mem`-class result, `returns_twice`, variadic calls (`al`) and
/// definitions (`va_start`, `va_copy`, `va_end` read by gcc's
/// `va_arg`), `musttail` (exchanged stack arguments, variadic thunks,
/// through a pointer), `tail` and `notail` indirect calls, `noreturn`.
/// `c_clobber` fills every argument and scratch register in between.
#[test]
fn handwritten_calls_run() {
    let dir = Scratch::new("x64-call-hand");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let ll = data("hand.ll");
    let c = data("hand.c");
    let flags = ["-fPIC", "-w"];
    let hod_driver =
        toolchain.compile_c(&dir, "driver-hod", &c, &[&flags[..], &["-DHOD"]].concat());
    let llc_driver = toolchain.compile_c(&dir, "driver-llc", &c, &flags);
    for (tag, with_llc, options, link) in builds() {
        let Some(obj) = object_with(&dir, &format!("hand-{tag}"), &ll, with_llc, options) else {
            return;
        };
        let driver = if with_llc { &llc_driver } else { &hod_driver };
        let name = format!("hand-{tag}");
        link_run_all(&toolchain, &dir, &name, &[link], &[driver, &obj], "done:", tag == "llc");
        if let Some(bytes) = text_bytes(&obj) {
            eprintln!("hand-{tag}: {bytes} bytes of .text");
        }
    }
}

/// The size of an object's `.text`, by `llvm-readelf`, if it is installed.
fn text_bytes(object: &Path) -> Option<u64> {
    let readelf = tool("llvm-readelf")?;
    let out = hod_testutil::run(readelf, [std::ffi::OsStr::new("-SW"), object.as_os_str()]);
    let line = out.stdout_text().lines().find(|line| line.contains(" .text "))?.to_owned();
    let fields: Vec<&str> = line.split_whitespace().collect();
    let at = fields.iter().position(|field| *field == ".text")?;
    u64::from_str_radix(fields.get(at + 4)?, 16).ok()
}

/// Real C++ exceptions through hod's frames (`tests/data/call/eh.ll`,
/// driven by `eh.cpp` built with g++): cleanups and `resume`, a catch by
/// type, a rethrow from a handler, nested pads in nested frames, two
/// clauses, a satisfied filter, a `noreturn` thrower, an `invoke` with
/// stack arguments, a frame without a personality, a throw from hod, a
/// function in a COMDAT group defined in two objects (`eh_comdat.ll`).
/// Static and stressed under every linker, then position-independent in
/// a shared object under every linker; `llc`'s objects are the control.
#[test]
fn exceptions_unwind_through_hod_frames() {
    let dir = Scratch::new("x64-call-eh");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let Some(driver) = toolchain.compile_cxx(&dir, "eh-driver", &data("eh.cpp"), &["-fPIC"]) else {
        return;
    };
    let (eh, eh_comdat) = (data("eh.ll"), data("eh_comdat.ll"));
    let stress = Options { stress: Stress::ALL, ..Options::default() };
    let rpath = format!("-Wl,-rpath,{}", dir.path().display());
    // After the objects, so that `--as-needed` keeps it.
    let stdcxx = Path::new("-lstdc++");
    for (tag, with_llc, options, shared) in [
        ("llc", true, Options::default(), false),
        ("hod", false, Options::default(), false),
        ("stress", false, stress, false),
        ("pic", false, pic(), true),
        ("llc-pic", true, pic(), true),
    ] {
        let Some(obj) = object_with(&dir, &format!("eh-{tag}"), &eh, with_llc, options) else {
            return;
        };
        let comdat = object_with(&dir, &format!("ehc-{tag}"), &eh_comdat, with_llc, options)
            .expect("the second object");
        if let Some(readelf) = tool("llvm-readelf") {
            let groups =
                hod_testutil::run(readelf, [std::ffi::OsStr::new("-gW"), comdat.as_os_str()])
                    .stdout_text();
            assert!(groups.contains("[h_comdat]"), "eh-{tag}: no group\n{groups}");
            if !with_llc {
                assert!(groups.contains("gcc_except_table"), "eh-{tag}: LSDA outside\n{groups}");
            }
        }
        let name = format!("eh-{tag}");
        if !shared {
            let inputs: [&Path; 4] = [&driver, &obj, &comdat, stdcxx];
            link_run_all(&toolchain, &dir, &name, &["-no-pie"], &inputs, "eh done", false);
            continue;
        }
        for linker in &toolchain.linkers {
            let lib = dir.join(format!("libeh-{tag}-{}.so", linker.name));
            toolchain.link_ok(linker, &["-shared"], &[&obj, &comdat], &lib);
            let exe = dir.join(format!("{name}-{}", linker.name));
            let flags = ["-rdynamic", &rpath];
            let ran = toolchain.link_and_run(linker, &flags, &[&driver, &lib, stdcxx], &exe);
            let out = ran.stdout_text();
            assert!(
                exit_code(&ran) == Some(0) && out.contains("eh done"),
                "{name} {}: exit {:?}\n{out}{}",
                linker.name,
                exit_code(&ran),
                ran.stderr_text()
            );
        }
    }
}

/// The forms this group refuses, by name (`spec/codegen.md` §5.6.1).
#[test]
fn the_remaining_forms_are_refused_by_name() {
    for (text, name) in [
        ("define i64 @f(ptr %ap) {\n  %r = va_arg ptr %ap, i64\n  ret i64 %r\n}\n", call::VA_ARG),
        (
            "declare void @llvm.va_start.p0(ptr)\n\
             define void @f(ptr %a) {\n  call void @llvm.va_start.p0(ptr %a)\n  ret void\n}\n",
            call::VA_INTRINSIC,
        ),
        (
            "declare void @g()\n\
             define void @f() {\n  call void @g() [ \"deopt\"() ]\n  ret void\n}\n",
            call::CALL_BUNDLE,
        ),
    ] {
        assert_eq!(compile(text), Err(CgError::unsupported(name)), "{text}");
    }
    // Lowered: extended odd widths in registers and on the stack, a wide
    // extended integer, a `byval` taken from the declaration, `sret`.
    for text in [
        "declare void @g(i7 zeroext)\ndefine void @f(i7 %x) {\n  call void @g(i7 zeroext %x)\n  ret void\n}\n",
        "declare void @g(i64, i64, i64, i64, i64, i64, i7 signext)\n\
         define void @f(i7 %x) {\n  call void @g(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i7 signext %x)\n  ret void\n}\n",
        "declare void @g(i100)\ndefine void @f(i100 %x) {\n  call void @g(i100 signext %x)\n  ret void\n}\n",
        "declare void @g(ptr byval([5 x i64]) align 8)\ndefine void @f(ptr %p) {\n  call void @g(ptr %p)\n  ret void\n}\n",
        "define void @f(ptr sret(i64) %r) {\n  ret void\n}\n",
    ] {
        assert_eq!(compile(text), Ok(()), "{text}");
    }
}

/// A result wider than 64 bits with `signext` is owed extended to 128
/// bits (`spec/abi-x64.md` §7): its last piece is extended.
#[test]
fn a_wide_signext_return_is_extended() {
    let dir = Scratch::new("x64-call-wide-ret");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let text = "define signext i65 @f(i65 %x) {\n  ret i65 %x\n}\n";
    let obj = object_with(&dir, "wide", text, false, pic()).expect("hod");
    let c = "#include <stdio.h>\n__int128 f(__int128);\nint main(void) {\n\
             __int128 r = f(((__int128)1 << 64) | 5);\n\
             int ok = (unsigned long long)(r >> 64) == ~0ULL;\n\
             printf(\"done\\n\");\n  return !ok;\n}\n";
    let driver = toolchain.compile_c(&dir, "driver", c, &["-fPIC"]);
    link_run_all(&toolchain, &dir, "wide", &["-pie"], &[&driver, &obj], "done", true);
}

/// The scalar types of the chains: integers of the widths whose
/// extension and pieces differ, and every floating-point type.
fn chain_scalars() -> Vec<ty::Ty> {
    use ty::Ty;
    let mut all: Vec<Ty> =
        [1, 7, 8, 16, 17, 32, 33, 63, 64, 65, 96, 128].iter().map(|&b| Ty::Int(b)).collect();
    all.extend([Ty::Ptr, Ty::Half, Ty::BFloat, Ty::Float, Ty::Double, Ty::Fp128, Ty::X86Fp80]);
    all.extend([ty::vec(Ty::Float, 4), ty::vec(ty::I32, 2), ty::vec(Ty::Double, 2)]);
    all.extend([ty::vec(ty::I8, 16), ty::vec(ty::I64, 2), ty::vec(Ty::Half, 3)]);
    all
}

fn chain_type(rng: &mut ty::Rng, pool: &[ty::Ty], depth: u32) -> ty::Ty {
    let roll = rng.below(100);
    if depth < 2 && roll >= 85 {
        let bound = if rng.chance(4) { 8 } else { 3 };
        let len = 1 + rng.below(bound);
        return ty::arr(chain_type(rng, pool, depth + 1), len);
    }
    if depth < 2 && roll >= 65 {
        let fields = (0..=rng.below(4)).map(|_| chain_type(rng, pool, depth + 1)).collect();
        return if rng.chance(8) { ty::Ty::Packed(fields) } else { ty::Ty::Struct(fields) };
    }
    rng.pick(pool).clone()
}

/// One random `ccc` signature of fixed arity: integers with and without
/// extensions, floating point, vectors, aggregates, `byval`, `sret`.
fn chain_case(rng: &mut ty::Rng, pool: &[ty::Ty]) -> sigs::Case {
    use sigs::{Attr, pa};
    let bound = if rng.chance(3) { 20 } else { 9 };
    let count = rng.below(bound) as usize;
    let sret = count > 0 && rng.chance(10);
    let params = (0..count)
        .map(|index| {
            let roll = rng.below(100);
            if sret && index == 0 {
                pa(ty::Ty::Ptr, Attr::Sret)
            } else if roll < 8 {
                let align = *rng.pick(&[None, Some(1), Some(8), Some(16)]);
                sigs::byval(chain_type(rng, pool, 0), align, None)
            } else {
                let ty = chain_type(rng, pool, 0);
                if ty.int_bits().is_some() && roll < 50 {
                    pa(ty, Attr::Ext(rng.chance(2)))
                } else {
                    sigs::p(ty)
                }
            }
        })
        .collect();
    let case = sigs::Case::new(params);
    if sret || rng.chance(4) {
        return case;
    }
    let ty = chain_type(rng, pool, 0);
    if ty.int_bits().is_some() && rng.chance(2) {
        case.ret_ext(ty, rng.chance(2))
    } else {
        case.ret(ty)
    }
}

/// The hod forwarder of case `index`: `hf_<index>` receives the
/// signature's arguments, calls `c_clobber`, passes every argument on to
/// `lf_<index>` (with the same attributes), calls `c_clobber` again and
/// returns what came back. `hc_<index>` is the driver's empty step.
fn forwarder(index: usize, case: &sigs::Case) -> String {
    let mut params = Vec::new();
    let mut args = Vec::new();
    for (n, param) in case.params.iter().enumerate() {
        params.push(format!("{}{} %a{n}", param.ty.text(), param.attr_text()));
        args.push(format!("{}{} %a{n}", param.ty.text(), param.attr_text()));
    }
    let declared: Vec<String> = case
        .params
        .iter()
        .map(|param| format!("{}{}", param.ty.text(), param.attr_text()))
        .collect();
    let (ret, ret_ty) = (case.ret_text(), case.ret.as_ref().map(ty::Ty::text));
    let (params, args, declared) = (params.join(", "), args.join(", "), declared.join(", "));
    let call = format!("call {ret} @lf_{index}({args})");
    let body = match ret_ty {
        Some(ty) => format!("  %r = {call}\n  call void @c_clobber()\n  ret {ty} %r\n"),
        None => format!("  {call}\n  call void @c_clobber()\n  ret void\n"),
    };
    format!(
        "declare {ret} @lf_{index}({declared})\n\
         define {ret} @hf_{index}({params}) {{\n  call void @c_clobber()\n{body}}}\n\
         define void @hc_{index}() {{\n  ret void\n}}\n"
    )
}

/// `c_clobber` in assembly: junk in every argument and scratch register.
const CLOBBER: &str = "\t.globl c_clobber\n\t.type c_clobber,@function\nc_clobber:\n\
    \tmovabsq $0x5a5a5a5a5a5a5a5a, %rdi\n\tmovq %rdi, %rsi\n\tmovq %rdi, %rdx\n\
    \tmovq %rdi, %rcx\n\tmovq %rdi, %r8\n\tmovq %rdi, %r9\n\tmovq %rdi, %r10\n\
    \tmovq %rdi, %r11\n\tmovq %rdi, %rax\n\
    \tpcmpeqd %xmm0, %xmm0\n\tpcmpeqd %xmm1, %xmm1\n\tpcmpeqd %xmm2, %xmm2\n\
    \tpcmpeqd %xmm3, %xmm3\n\tpcmpeqd %xmm4, %xmm4\n\tpcmpeqd %xmm5, %xmm5\n\
    \tpcmpeqd %xmm6, %xmm6\n\tpcmpeqd %xmm7, %xmm7\n\tret\n";

/// Mixed objects (WP-118's signature machinery, `tests/abi`): `llc`'s
/// caller `lc_k` calls hod's `hf_k`, which forwards every argument to
/// `llc`'s callee `lf_k` and returns its result, with `c_clobber`
/// between. What `lf_k` records must be every argument as `lc_k` passed
/// it, extended as LLVM believes (`hod` as a caller); what `lc_k` records
/// must be `lf_k`'s result (`hod` as a callee handing a result on). Random
/// `ccc` signatures `assign` accepts, against `llc` 22 at `-O0` and `-O2`;
/// a result in memory (WP-126) and an x87 part (WP-131) are left out.
#[test]
fn hod_forwards_random_signatures_between_llc_frames() {
    const COUNT: usize = 300;
    let Some(tools) = run::Tools::find() else { return };
    let mut rng = ty::Rng(0x0125_c0de);
    let mut types = sigs::table();
    let mut blobs = sigs::Blobs::default();
    let pool = chain_scalars();
    let mut plans = Vec::new();
    let mut hod_text = String::from("declare void @c_clobber()\n");
    let mut left_out = std::collections::BTreeMap::<&str, usize>::new();
    while plans.len() < COUNT {
        let case = chain_case(&mut rng, &pool);
        let asg = match case.assign(&mut types) {
            Ok(asg) => asg,
            Err(name) => {
                *left_out.entry(name).or_insert(0) += 1;
                continue;
            }
        };
        let x87 = asg.ret.iter().any(
            |part| matches!(part.place, hod_cg::abi::Place::Reg(r) if hod_x64::abi::is_x87(r)),
        );
        if asg.ret_memory.is_some() || x87 {
            *left_out.entry(if x87 { "x87 result" } else { "result in memory" }).or_insert(0) += 1;
            continue;
        }
        let index = plans.len();
        let text = forwarder(index, &case);
        let alone = format!("declare void @c_clobber()\n{text}");
        match compile(&alone) {
            Ok(()) => {}
            // `X64` assigns as if AVX were on (WP-118's follow-up).
            Err(CgError::Unsupported { construct: "x64:wide-vector-abi" }) => {
                *left_out.entry("x64:wide-vector-abi").or_insert(0) += 1;
                continue;
            }
            Err(other) => panic!("{other:?}\n{text}"),
        }
        hod_text.push_str(&text);
        plans.push(sigs::Plan::new(&case, &mut types, &mut blobs, &mut rng));
    }
    eprintln!("chains: {COUNT} signatures; left out {left_out:?}");
    let mut asm = String::new();
    emit::prelude(&mut asm);
    asm.push_str(CLOBBER);
    emit::epilogue(&mut asm);
    let harness = run::Harness::new("x64-call-chains", blobs, COUNT, &asm, &tools);
    let scratch = &harness.scratch;
    let (module, bodies) = parse(&hod_text);
    let hod = hod_x64::compile_to_object(&module, &bodies, &pic()).expect("the forwarders");
    let hod = scratch.write("forwarders.o", hod);
    let llvm = scratch.write("llvm.ll", ir::module(&plans, &|_, _| true));
    let size = harness.blobs.out_size as usize;
    let mut checked = 0;
    let configs = tools.llc.iter().filter(|(c, _)| c.major == 22 && (c.dag || c.opt == "-O2"));
    for (config, llc) in configs {
        let object = scratch.join(format!("llvm-{}.o", config.tag()));
        run::must("llc", &run::compile(llc, *config, &llvm, &object));
        let wanted = vec![true; 2 * COUNT];
        let (out, crashed) = harness
            .execute(&config.tag(), &[object, hod.clone()], &wanted, &tools)
            .unwrap_or_else(|(why, _)| panic!("{}: {why}", config.tag()));
        let mut problems = Vec::new();
        for (index, plan) in plans.iter().enumerate() {
            if crashed[2 * index + 1] {
                problems.push(format!("case {index} crashed: `{}`", plan.case.text()));
                continue;
            }
            let mut args = plan.clone();
            args.ret = None;
            let mut ret = plan.clone();
            ret.args.clear();
            let found = run::compare(&args, true, &out[..size])
                .or_else(|| run::compare(&ret, false, &out[size..]));
            if let Some(problem) = found {
                problems.push(format!("case {index} `{}`: {problem}", plan.case.text()));
            }
            checked += 1;
        }
        assert!(problems.is_empty(), "llc {}: {}", config.tag(), problems.join("\n"));
    }
    eprintln!("chains: {checked} runs compared");
}
