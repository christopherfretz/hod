//! The intrinsic group's own programs (WP-041): a systematic table of
//! one-intrinsic functions (every integer intrinsic at every width, with
//! register and constant operands, and the poison flags), and random
//! straight-line functions that chain them with integer arithmetic. Both
//! are `.ll` text built on `int_gen`'s [`Suite`], with the `declare`s
//! they need collected in [`Gen::decls`], so the same functions go to
//! `hod_interp`, to `hod_a64` and to `llc`.
//!
//! [`run`] is `int_gen::run` with the declarations in the header: every
//! case runs in the interpreter and natively from one C driver, compiled
//! by hod plainly and with every stress option, and by `llc -O0
//! -global-isel=0 -fast-isel=0`. A case the interpreter traps on (a
//! poison flag that is set and hit) is run but not compared.

#![allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
#![allow(dead_code, reason = "each test uses part of it")]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::process::Command;
use std::time::{Duration, Instant};

use hod_a64::compile_module;
use hod_cg::{Options, Stress};
use hod_interp::{Interp, Program, Val};
use hod_testutil::{Scratch, run_with};

use crate::common::linking::Toolchain;
use crate::common::{ll_header, llc, llc_major, parse, write_object};
use crate::int_gen::{Report, Rng, Suite, Ty, edges, lit, mask, value};

/// The widths every intrinsic is checked at.
pub const WIDTHS: [u32; 18] = [1, 2, 3, 7, 8, 9, 15, 16, 17, 24, 31, 32, 33, 40, 48, 56, 63, 64];

/// The unary intrinsics, with the poison flag argument they take.
pub const UNARY: [&str; 6] = ["ctlz", "cttz", "ctpop", "bswap", "bitreverse", "abs"];
/// The binary ones.
pub const BINARY: [&str; 4] = ["smax", "smin", "umax", "umin"];
/// The funnel shifts.
pub const FUNNEL: [&str; 2] = ["fshl", "fshr"];

/// Whether intrinsic `name` is defined at `bits` (`bswap`: whole 16-bit
/// halves).
pub fn defined_at(name: &str, bits: u32) -> bool {
    name != "bswap" || bits % 16 == 0
}

/// Whether `name` takes the `i1` poison flag.
fn has_flag(name: &str) -> bool {
    matches!(name, "ctlz" | "cttz" | "abs")
}

/// A suite and the declarations its functions need.
#[derive(Default, Debug)]
pub struct Gen {
    /// The functions and cases.
    pub suite: Suite,
    /// The `declare` lines, each once.
    pub decls: BTreeSet<String>,
}

impl Gen {
    /// `call iN @llvm.<name>.iN(<args>)`, declaring it.
    fn call(&mut self, name: &str, bits: u32, args: &[String], flag: Option<bool>) -> String {
        let t = format!("i{bits}");
        let mut params: Vec<String> = args.iter().map(|_| t.clone()).collect();
        let mut list: Vec<String> = args.iter().map(|a| format!("{t} {a}")).collect();
        if let Some(flag) = flag {
            params.push("i1".into());
            list.push(format!("i1 {flag}"));
        }
        self.decls.insert(format!("declare {t} @llvm.{name}.{t}({})", params.join(", ")));
        format!("call {t} @llvm.{name}.{t}({})", list.join(", "))
    }

    /// The module header: target lines and declarations.
    pub fn header(&self, major: u32) -> String {
        let mut h = ll_header(major);
        for d in &self.decls {
            h.push_str(d);
            h.push('\n');
        }
        h
    }
}

/// Every intrinsic at every width: each unary one of a register (both
/// poison flags where there is one), each binary one of two registers
/// and of a register and a constant, each funnel shift of three
/// registers and with each of a set of constant amounts.
pub fn table(rng: &mut Rng) -> Gen {
    let mut g = Gen::default();
    for &bits in &WIDTHS {
        let ty = Ty::I(bits);
        let t = ty.ll();
        let edge = edges(bits);
        for name in UNARY {
            if !defined_at(name, bits) {
                continue;
            }
            let flags: &[Option<bool>] =
                if has_flag(name) { &[Some(false), Some(true)] } else { &[None] };
            for &flag in flags {
                let call = g.call(name, bits, &["%p0".into()], flag);
                let fname =
                    format!("u_{name}_{bits}{}", if flag == Some(true) { "_p" } else { "" });
                let f = g.suite.add(fname, &[ty], ty, &format!("  %r = {call}\n  ret {t} %r\n"));
                for &v in &edge {
                    g.suite.case(f, [v, 0, 0]);
                }
                for _ in 0..8 {
                    g.suite.case(f, [rng.next() & mask(bits), 0, 0]);
                }
            }
        }
        for name in BINARY {
            let call = g.call(name, bits, &["%p0".into(), "%p1".into()], None);
            let f = g.suite.add(
                format!("b_{name}_{bits}"),
                &[ty, ty],
                ty,
                &format!("  %r = {call}\n  ret {t} %r\n"),
            );
            for &a in &edge {
                for _ in 0..3 {
                    let b = *rng.pick(&edge);
                    g.suite.case(f, [a, b, 0]);
                }
                g.suite.case(f, [a, a, 0]);
            }
            for _ in 0..10 {
                g.suite.case(f, [rng.next() & mask(bits), rng.next() & mask(bits), 0]);
            }
            // A constant on the right, and on the left.
            for (k, &c) in [0, 1, mask(bits), 1 << (bits - 1)].iter().enumerate() {
                let right = g.call(name, bits, &["%p0".into(), lit(ty, c)], None);
                let left = g.call(name, bits, &[lit(ty, c), "%p0".into()], None);
                let f = g.suite.add(
                    format!("b_{name}_{bits}_k{k}"),
                    &[ty],
                    ty,
                    &format!(
                        "  %a = {right}\n  %b = {left}\n  %r = xor {t} %a, %b\n  ret {t} %r\n"
                    ),
                );
                for &v in &edge {
                    g.suite.case(f, [v, 0, 0]);
                }
            }
        }
        for name in FUNNEL {
            let call = g.call(name, bits, &["%p0".into(), "%p1".into(), "%p2".into()], None);
            let f = g.suite.add(
                format!("f_{name}_{bits}"),
                &[ty, ty, ty],
                ty,
                &format!("  %r = {call}\n  ret {t} %r\n"),
            );
            let amounts: Vec<u64> = (0..=u64::from(bits) + 2)
                .chain([2 * u64::from(bits) + 3, mask(bits), mask(bits) - 1])
                .map(|v| v & mask(bits))
                .collect();
            for &c in &amounts {
                for _ in 0..2 {
                    g.suite.case(f, [value(rng, ty), value(rng, ty), c]);
                }
            }
            for _ in 0..10 {
                g.suite.case(f, [value(rng, ty), value(rng, ty), rng.next() & mask(bits)]);
            }
            // Constant amounts.
            let mut constant: Vec<u64> = vec![0, 1, 2, u64::from(bits) - 1];
            constant.extend([u64::from(bits), u64::from(bits) + 1, 2 * u64::from(bits) + 3]);
            constant.push(rng.next());
            for (k, &c) in constant.iter().enumerate() {
                let c = c & mask(bits);
                let call = g.call(name, bits, &["%p0".into(), "%p1".into(), lit(ty, c)], None);
                let f = g.suite.add(
                    format!("f_{name}_{bits}_k{k}"),
                    &[ty, ty],
                    ty,
                    &format!("  %r = {call}\n  ret {t} %r\n"),
                );
                for _ in 0..12 {
                    g.suite.case(f, [value(rng, ty), value(rng, ty), 0]);
                }
            }
            // A rotate: the same value twice.
            let call = g.call(name, bits, &["%p0".into(), "%p0".into(), "%p1".into()], None);
            let f = g.suite.add(
                format!("f_{name}_{bits}_rot"),
                &[ty, ty],
                ty,
                &format!("  %r = {call}\n  ret {t} %r\n"),
            );
            for _ in 0..12 {
                g.suite.case(f, [value(rng, ty), value(rng, ty), 0]);
            }
        }
    }
    g
}

/// Random straight-line functions of one width: each step applies a
/// random intrinsic (or `add`, `xor`, `mul` with a constant) to values
/// of the pool, the parameters and the earlier results; the result is
/// the xor of the last four. `count` functions.
pub fn random(rng: &mut Rng, count: usize) -> Gen {
    let mut g = Gen::default();
    for n in 0..count {
        let bits = *rng.pick(&WIDTHS);
        let ty = Ty::I(bits);
        let t = ty.ll();
        let params = vec![ty; 1 + rng.below(3) as usize];
        let mut pool: Vec<String> = (0..params.len()).map(|i| format!("%p{i}")).collect();
        let mut body = String::new();
        let steps = 2 + rng.below(12);
        for step in 0..steps {
            let pick = |rng: &mut Rng| -> String {
                if rng.chance(15) { lit(ty, value(rng, ty)) } else { rng.pick(&pool).clone() }
            };
            let inst = match rng.below(16) {
                0..=5 => {
                    let name = loop {
                        let name = *rng.pick(&UNARY);
                        if defined_at(name, bits) {
                            break name;
                        }
                    };
                    let arg = rng.pick(&pool).clone();
                    let flag = has_flag(name).then_some(false);
                    g.call(name, bits, &[arg], flag)
                }
                6..=9 => {
                    let name = *rng.pick(&BINARY);
                    let (a, b) = (pick(rng), pick(rng));
                    g.call(name, bits, &[a, b], None)
                }
                10..=12 => {
                    let name = *rng.pick(&FUNNEL);
                    let (a, b, c) = (rng.pick(&pool).clone(), pick(rng), pick(rng));
                    g.call(name, bits, &[a, b, c], None)
                }
                13 => format!("add {t} {}, {}", rng.pick(&pool), pick(rng)),
                14 => format!("xor {t} {}, {}", rng.pick(&pool), pick(rng)),
                _ => format!("mul {t} {}, {}", rng.pick(&pool), lit(ty, rng.next())),
            };
            writeln!(body, "  %v{step} = {inst}").unwrap();
            pool.push(format!("%v{step}"));
        }
        let mut acc = pool.last().cloned().unwrap_or_default();
        for (k, v) in pool.iter().rev().skip(1).take(3).enumerate() {
            writeln!(body, "  %x{k} = xor {t} {acc}, {v}").unwrap();
            acc = format!("%x{k}");
        }
        writeln!(body, "  ret {t} {acc}").unwrap();
        let f = g.suite.add(format!("r{n}"), &params, ty, &body);
        for _ in 0..6 {
            let mut args = [0; 3];
            for arg in args.iter_mut().take(params.len()) {
                *arg = value(rng, ty);
            }
            g.suite.case(f, args);
        }
    }
    g
}

// ---- running (`int_gen::run`, with the declarations) ----

/// The C driver: every case in a table, run in order, each result
/// printed in hex at its width; arguments carry random bits above their
/// width.
fn driver(suite: &Suite, rng: &mut Rng) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n\
         typedef uint64_t (*fn_t)(uint64_t, uint64_t, uint64_t);\n",
    );
    for f in &suite.funcs {
        writeln!(c, "uint64_t {}(uint64_t, uint64_t, uint64_t);", f.name).unwrap();
    }
    c.push_str("static const struct { fn_t f; uint64_t mask, a, b, c; } cases[] = {\n");
    for case in &suite.cases {
        let f = &suite.funcs[case.func];
        let mut regs = [0u64; 3];
        for (i, &ty) in f.params.iter().enumerate() {
            let garbage = rng.next();
            regs[i] = match ty {
                Ty::Ptr => case.args[i],
                Ty::I(1) => garbage & !0xff | case.args[i] & 1,
                Ty::I(bits) => garbage & !mask(bits) | case.args[i],
            };
        }
        let result_mask = if f.ret == Ty::I(1) { 0xff } else { mask(f.ret.bits()) };
        writeln!(
            c,
            "  {{ {}, {result_mask:#x}ULL, {:#x}ULL, {:#x}ULL, {:#x}ULL }},",
            f.name, regs[0], regs[1], regs[2]
        )
        .unwrap();
    }
    c.push_str(
        "};\nint main(void) {\n  for (unsigned long i = 0; i < sizeof cases / sizeof cases[0]; i++) {\n    \
         uint64_t r = cases[i].f(cases[i].a, cases[i].b, cases[i].c) & cases[i].mask;\n    \
         printf(\"%llx\\n\", (unsigned long long)r);\n  }\n  return 0;\n}\n",
    );
    c
}

/// The interpreter's result of each case; `None` where it traps.
fn interpret(suite: &Suite, header: &str) -> Vec<Option<u64>> {
    let mut expected = vec![None; suite.cases.len()];
    let mut by_func: Vec<Vec<usize>> = vec![Vec::new(); suite.funcs.len()];
    for (index, case) in suite.cases.iter().enumerate() {
        by_func[case.func].push(index);
    }
    for (chunk, funcs) in suite.funcs.chunks(64).enumerate() {
        let mut text = header.to_string();
        for f in funcs {
            text.push_str(&suite.text[f.text.clone()]);
        }
        let (module, bodies) = parse(&text);
        let program = Program::with_bodies(&module, &bodies).expect("a valid program");
        let mut interp = Interp::new(&program);
        for (offset, f) in funcs.iter().enumerate() {
            let sym = module.find_symbol(f.name.as_bytes()).expect("a defined function");
            for &index in &by_func[chunk * 64 + offset] {
                let args: Vec<Val> = f
                    .params
                    .iter()
                    .zip(suite.cases[index].args)
                    .map(|(&ty, v)| match ty {
                        Ty::Ptr => Val::Ptr(v),
                        Ty::I(bits) => Val::int(bits, u128::from(v)),
                    })
                    .collect();
                expected[index] = match interp.run(sym, &args).result {
                    Ok(Some(Val::Int { value, .. })) => Some(value as u64),
                    Ok(other) => panic!("{}: {other:?}", f.name),
                    Err(_) => None,
                };
            }
        }
    }
    expected
}

/// Runs `program` and parses one hex number per line.
fn lines(program: &std::path::Path, what: &str) -> Vec<u64> {
    let ran = run_with(&mut Command::new(program), &[], Duration::from_mins(2));
    assert!(ran.success(), "{what}: {:?}\n{}", ran.status, ran.stderr_text());
    ran.stdout_text()
        .lines()
        .map(|line| u64::from_str_radix(line, 16).unwrap_or_else(|_| panic!("{what}: {line}")))
        .collect()
}

/// Checks `g`: the interpreter against hod (plain, stressed) and `llc`.
pub fn run(name: &str, g: &Gen) -> Report {
    let suite = &g.suite;
    let major = llc_major();
    let header = g.header(major);
    let text = format!("{header}{}", suite.text);
    let (module, bodies) = parse(&text);
    let mut report =
        Report { insts: bodies.iter().map(hod_ir::Body::inst_count).sum(), ..Report::default() };
    let start = Instant::now();
    let expected = interpret(suite, &header);
    report.interp = start.elapsed();
    let start = Instant::now();
    report.undefined = expected.iter().filter(|e| e.is_none()).count();
    report.compared = expected.len() - report.undefined;

    let scratch = Scratch::new(&format!("a64-intrinsic-{name}"));
    let mut legs: Vec<(String, std::path::PathBuf)> = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let options = Options { stress, ..Options::default() };
        let t = Instant::now();
        let emitted =
            compile_module(&module, &bodies, &options).unwrap_or_else(|e| panic!("{mode}: {e}"));
        if mode == "plain" {
            report.compile = t.elapsed();
        }
        let object = write_object(&scratch, &format!("hod-{mode}.o"), &emitted);
        legs.push((format!("hod {mode}"), object));
    }
    if let Some(object) = llc(&scratch, "llc", &text, &[]) {
        legs.push(("llc".to_string(), object));
    }
    let Some(toolchain) = Toolchain::host(&scratch) else { return report };
    let Some(linker) = toolchain.linkers.first() else { return report };
    let mut rng = Rng(0x0041_0041);
    let driver = toolchain.compile_c(&scratch, "driver", &driver(suite, &mut rng), &[]);
    for (leg, object) in &legs {
        let program = scratch.join(leg.replace(' ', "-"));
        toolchain.link_ok(linker, &[], &[&driver, object], &program);
        let got = lines(&program, leg);
        assert_eq!(got.len(), suite.cases.len(), "{leg}: one line per case");
        for ((case, want), got) in suite.cases.iter().zip(&expected).zip(&got) {
            let f = &suite.funcs[case.func];
            if let Some(want) = want
                && want != got
            {
                report.wrong.push(format!(
                    "{leg}: {}({:#x}, {:#x}, {:#x}): interpreter {want:#x}, native {got:#x}",
                    f.name, case.args[0], case.args[1], case.args[2]
                ));
            }
        }
    }
    report.tools = start.elapsed();
    report
}
