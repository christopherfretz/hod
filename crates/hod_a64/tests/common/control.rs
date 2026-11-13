//! WP-037's generator and harness (included by `tests/control.rs` as
//! its module `graphs`, not by `common/mod.rs`): random control-flow graphs whose only
//! instructions are `phi`s and terminators (`br`, `condbr`, `switch`,
//! `indirectbr`, `ret`), as `.ll` text, and the check that runs a batch
//! of them in `hod_interp` and natively (hod, and optionally `llc`).
//!
//! Every generated function terminates: its blocks form a DAG in index
//! order except for loops of two fixed shapes whose exit depends on the
//! arguments only through the initial value of a state `phi`:
//!
//! - a **header loop**: the header `switch`es on its state `phi` to case
//!   blocks that branch back with a constant next state, each case block
//!   only to the state of a later case block or to one no case takes;
//!   the header's rotation `phi`s read each other on the back edges (the
//!   swap problem);
//! - a **self loop**: a block that branches to itself at most once, by a
//!   `switch` whose back edge sets the state to a value that leaves, or a
//!   `condbr` on an `i1` `phi` that is `false` on the back edge; a pair of
//!   its `phi`s swap.
//!
//! What a function returns is one of its values (mostly a `phi`), so the
//! result names the path taken; the `switch`es and `condbr`s read
//! arguments and `phi`s of every width, so a wrong `phi` value changes
//! the path too.

#![allow(
    clippy::needless_range_loop,
    clippy::range_plus_one,
    clippy::struct_excessive_bools,
    reason = "graph code over block indexes reads clearer by index; the options are independent switches"
)]

use std::fmt::Write as _;
use std::process::Command;
use std::time::Duration;

use hod_a64::compile_module;
use hod_cg::Options;
use hod_interp::{Interp, Program, Val};
use hod_testutil::{Scratch, run_with};

use crate::common::linking::Toolchain;
use crate::common::{ll_header, llc, llc_major, parse, write_object};

#[path = "../../../hod_testutil/gen/control_gen.rs"]
mod graphs_gen;
pub use graphs_gen::*;

// ---- the check ----

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
/// parameters (AAPCS64 passes them in `x0`..`x5` whatever the callee
/// declares), the arguments with random bits above their width (§2.2:
/// the callee may not rely on them; an `i1` is extended to 8 bits by its
/// producer, `abi-a64.md` §7), each result masked to its width.
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
                Ty::Int(1) => v | (junk << 8),
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
    /// The mode (`plain`, `stress`, `llc`, ...).
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

/// Runs every call of `calls` in the interpreter, and natively compiled
/// by hod in each of `modes` (and by `llc` when `with_llc`), all in one
/// object, one driver and one process per mode. Returns the
/// disagreements; `None` without the tools.
pub fn check(
    label: &str,
    funcs: &[Func],
    calls: &[Call],
    modes: &[Mode],
    with_llc: bool,
    seed: u64,
) -> Option<Vec<Wrong>> {
    let body: String = funcs.iter().map(|f| f.text.as_str()).collect();
    let text = format!("{}{body}", ll_header(22));
    let (module, bodies) = parse(&text);
    let program = Program::with_bodies(&module, &bodies).expect("bodies of the module");
    let expected: Vec<String> =
        calls.iter().map(|c| interpret(&program, &module, funcs, c)).collect();
    let scratch = Scratch::new(label);
    let toolchain = Toolchain::host(&scratch)?;
    let linker = toolchain.linkers.first()?;
    let mut rng = Rng(seed ^ 0xd51e_57a6);
    let driver = toolchain.compile_c(&scratch, "driver", &driver(funcs, calls, &mut rng), &[]);
    let mut wrong = Vec::new();
    let mut objects: Vec<(&'static str, std::path::PathBuf)> = Vec::new();
    for mode in modes {
        let emitted = compile_module(&module, &bodies, &mode.options)
            .unwrap_or_else(|e| panic!("{}: {e}\n{text}", mode.name));
        objects.push((mode.name, write_object(&scratch, &format!("{}.o", mode.name), &emitted)));
    }
    if with_llc {
        let major = llc_major();
        let llc_text = format!("{}{body}", ll_header(major));
        if let Some(object) = llc(&scratch, "llc", &llc_text, &[]) {
            objects.push(("llc", object));
        }
    }
    for (name, object) in objects {
        let exe = scratch.join(name);
        toolchain.link_ok(linker, &[], &[&driver, &object], &exe);
        let ran = run_with(&mut Command::new(&exe), &[], Duration::from_mins(2));
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
