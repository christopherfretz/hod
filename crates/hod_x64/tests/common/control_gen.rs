//! WP-037's graph generator (shared with `hod_a64` since WP-130:
//! `crates/hod_testutil/gen/control_gen.rs`), with the check
//! rewritten for x86-64: random control-flow graphs whose only
//! instructions are `phi`s and terminators (`br`, `condbr`, `switch`,
//! `indirectbr`, `ret`), as `.ll` text, run in `hod_interp` and, compiled
//! by hod for x86-64 (and optionally by `llc` 19 and 22), linked with a C
//! driver by the cross toolchain and run in ONE emulated process per
//! object.
//!
//! Every generated function terminates: its blocks form a DAG in index
//! order except for loops of two fixed shapes whose exit depends on the
//! arguments only through the initial value of a state `phi` (see the
//! AArch64 file's documentation). What a function returns is one of its
//! values (mostly a `phi`), so the result names the path taken.

#![allow(
    dead_code,
    clippy::needless_range_loop,
    clippy::range_plus_one,
    clippy::struct_excessive_bools,
    reason = "graph code over block indexes reads clearer by index; the options are independent switches"
)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use hod_cg::Options;
use hod_interp::{Interp, Program, Val};
use hod_testutil::{Scratch, run_with};

use crate::linking::Toolchain;

#[path = "../../../hod_testutil/gen/control_gen.rs"]
mod graphs_gen;
pub use graphs_gen::*;

// ---- the check (x86-64) ----

/// The module header for x86-64 Linux.
pub fn ll_header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// What clang writes for `-fPIE`: `PIC Level` 2, `PIE Level` 2.
pub const PIE_FLAGS: &str = "\n!llvm.module.flags = !{!900, !901}\n\
    !900 = !{i32 8, !\"PIC Level\", i32 2}\n!901 = !{i32 7, !\"PIE Level\", i32 2}\n";

/// A module and its bodies from `.ll` text (the header is added).
pub fn parse(functions: &str) -> (hod_ir::Module, Vec<hod_ir::Body>) {
    let text = format!("{}{functions}{PIE_FLAGS}", ll_header());
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        let body = parsed
            .body
            .unwrap_or_else(|| panic!("hod_ll: unsupported {:?} in\n{text}", parsed.unsupported));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    (module, bodies)
}

/// `llc -O0 -fast-isel=0` of LLVM `major` for x86-64 PIC, of the `.ll`
/// functions `functions`, to an object; `None` when that `llc` is not
/// installed.
pub fn llc(dir: &Scratch, major: u32, name: &str, functions: &str) -> Option<PathBuf> {
    let llc = hod_testutil::find_tool(&format!("llc-{major}"))?;
    let input = dir.write(&format!("{name}.ll"), format!("{}{functions}", ll_header()));
    let output = dir.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-fast-isel=0", "-mtriple=x86_64-unknown-linux-gnu"])
        .args(["-relocation-model=pic", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llc-{major} failed:\n{}", outcome.stderr_text());
    Some(output)
}

/// One call: the function's index and its arguments (defined bits only).
#[derive(Clone, Debug)]
pub struct Call {
    /// Index into the batch's functions.
    pub func: usize,
    /// The arguments.
    pub args: Vec<u64>,
}

/// How a batch is compiled.
#[derive(Clone, Copy, Debug)]
pub struct Mode {
    /// Its name in reports.
    pub name: &'static str,
    /// The options.
    pub options: Options,
}

/// The result of `call` in the interpreter, as the driver prints it.
fn interpret(
    program: &Program<'_>,
    module: &hod_ir::Module,
    funcs: &[Func],
    call: &Call,
) -> String {
    let func = &funcs[call.func];
    let sym = module.find_symbol(func.name.as_bytes()).expect("the function");
    let args: Vec<Val> = func
        .params
        .iter()
        .zip(&call.args)
        .map(|(&ty, &v)| match ty {
            Ty::Int(bits) => Val::int(bits, u128::from(v)),
            Ty::Ptr => Val::Ptr(v),
        })
        .collect();
    let outcome = Interp::new(program).run(sym, &args);
    match outcome.result {
        Ok(Some(Val::Int { value, .. })) => format!("{}", value as u64),
        Ok(Some(Val::Ptr(v))) => format!("{v}"),
        other => panic!("{}: {other:?}\n{}", func.name, func.text),
    }
}

/// The C driver: a table of calls through a pointer of six `uint64_t`
/// parameters (SysV passes them in `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`
/// whatever the callee declares), the arguments with random bits above
/// their width (§2.2: the callee may not rely on them; an `i1` gets them
/// from bit 1 on), each result masked to its width.
fn driver(funcs: &[Func], calls: &[Call], rng: &mut Rng) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n\
         typedef uint64_t (*fn6)(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t);\n",
    );
    for f in funcs {
        writeln!(c, "uint64_t {}(void);", f.name).unwrap();
    }
    c.push_str("static const struct { fn6 f; uint64_t m; uint64_t a[6]; } calls[] = {\n");
    for call in calls {
        let f = &funcs[call.func];
        let mut args = [0u64; 6];
        for (i, (&ty, &v)) in f.params.iter().zip(&call.args).enumerate() {
            let junk = rng.next();
            args[i] = match ty {
                Ty::Int(bits) if bits < 64 => v | (junk << bits),
                _ => v,
            };
        }
        let list: Vec<String> = args.iter().map(|a| format!("{a}ULL")).collect();
        writeln!(c, "  {{ (fn6){}, {}ULL, {{ {} }} }},", f.name, f.ret.mask(), list.join(", "))
            .unwrap();
    }
    c.push_str(
        "};\nint main(void) {\n\
           for (unsigned i = 0; i < sizeof calls / sizeof calls[0]; i++) {\n\
             const uint64_t *a = calls[i].a;\n\
             printf(\"%llu\\n\", (unsigned long long)(calls[i].f(a[0], a[1], a[2], a[3], a[4], a[5]) & calls[i].m));\n\
           }\n  return 0;\n}\n",
    );
    c
}

/// A disagreement: the call, the function, what each side printed.
#[derive(Debug)]
pub struct Wrong {
    /// The mode (`plain`, `stress`, `llc19`, ...).
    pub mode: &'static str,
    /// The call's index.
    pub call: usize,
    /// The function's name.
    pub func: String,
    /// The interpreter's result.
    pub want: String,
    /// The native result.
    pub got: String,
}

/// Which `llc`s a check runs besides hod.
pub const LLC_MAJORS: [u32; 2] = [19, 22];

/// Runs every call of `calls` in the interpreter, and compiled for
/// x86-64 by hod in each of `modes` (and by `llc` 19 and 22 when
/// `with_llc`), each in one object, one driver and one emulated process.
/// Returns the disagreements; `None` without the tools.
pub fn check(
    label: &str,
    funcs: &[Func],
    calls: &[Call],
    modes: &[Mode],
    with_llc: bool,
    seed: u64,
) -> Option<Vec<Wrong>> {
    let body: String = funcs.iter().map(|f| f.text.as_str()).collect();
    let (module, bodies) = parse(&body);
    let program = Program::with_bodies(&module, &bodies).expect("bodies of the module");
    let expected: Vec<String> =
        calls.iter().map(|c| interpret(&program, &module, funcs, c)).collect();
    let scratch = Scratch::new(label);
    let toolchain = Toolchain::x86_64(&scratch)?;
    let linker = toolchain.linkers.first()?;
    let mut rng = Rng(seed ^ 0xd51e_57a6);
    let driver = toolchain.compile_c(&scratch, "driver", &driver(funcs, calls, &mut rng), &[]);
    let mut wrong = Vec::new();
    let mut objects: Vec<(&'static str, PathBuf)> = Vec::new();
    for mode in modes {
        let bytes = hod_x64::compile_to_object(&module, &bodies, &mode.options)
            .unwrap_or_else(|e| panic!("{}: {e}\n{body}", mode.name));
        objects.push((mode.name, scratch.write(&format!("{}.o", mode.name), bytes)));
    }
    if with_llc {
        for (major, name) in LLC_MAJORS.into_iter().zip(["llc19", "llc22"]) {
            if let Some(object) = llc(&scratch, major, name, &body) {
                objects.push((name, object));
            }
        }
    }
    for (name, object) in objects {
        let exe = scratch.join(name);
        toolchain.link_ok(linker, &[], &[&driver, &object], &exe);
        let ran = toolchain.run(linker, &exe);
        assert!(ran.success(), "{name}: {:?}\n{}", ran.status, ran.stderr_text());
        let out = ran.stdout_text();
        let got: Vec<&str> = out.lines().collect();
        assert_eq!(got.len(), expected.len(), "{name}: {out}");
        for (i, (want, got)) in expected.iter().zip(got).enumerate() {
            if want != got {
                wrong.push(Wrong {
                    mode: name,
                    call: i,
                    func: funcs[calls[i].func].name.clone(),
                    want: want.clone(),
                    got: got.to_owned(),
                });
            }
        }
    }
    Some(wrong)
}

/// The function named `name` among `funcs`, for a report.
pub fn text_of<'f>(funcs: &'f [Func], name: &str) -> &'f str {
    funcs.iter().find(|f| f.name == name).map_or("", |f| f.text.as_str())
}
