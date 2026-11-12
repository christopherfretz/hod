//! `spec/ll.md` §11.1, `spec/ir.md` §5.1: bodies parsed on several
//! threads, against forks of the frozen module, are the sequential
//! parser's bodies, and the module ends as the sequential parser leaves
//! it: every table, entry by entry, every id, every unsupported
//! construct, and the first error in file order.

mod common;

use std::fmt::Write as _;

use hod_ir::{AsmId, Body, ConstId, ExprId, LocId, Module, ReadStats, TypeId};
use hod_ll::{LlError, ParseOptions, ParsedModule, parse_bodies_on, parse_module};

const THREADS: [usize; 5] = [1, 2, 3, 8, 24];

/// What a reading made: the bodies (or their unsupported constructs) in
/// the order delivered, the error, and the module's tables.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    bodies: Vec<(u32, Result<Body, Vec<String>>)>,
    error: Option<LlError>,
    tables: String,
}

/// Every entry of every table bodies add to, by id.
fn tables(module: &Module) -> String {
    let mut out = String::new();
    for id in 0..module.types.len() {
        writeln!(out, "t{id} {:?}", module.types.kind(TypeId::new(id as u32))).unwrap();
    }
    for id in 0..module.consts.len() {
        writeln!(out, "c{id} {:?}", module.consts.get(ConstId::new(id as u32))).unwrap();
    }
    for id in 0..module.asm_count() {
        writeln!(out, "a{id} {:?}", module.asm(AsmId::new(id as u32))).unwrap();
    }
    writeln!(out, "fn attrs {}", module.fn_attrs_count()).unwrap();
    let debug = &module.debug;
    for id in 0..debug.expr_count() {
        writeln!(out, "e{id} {:?}", debug.expr_elements(ExprId::new(id as u32))).unwrap();
    }
    for id in 0..debug.loc_count() {
        let loc = LocId::new(id as u32);
        writeln!(out, "l{id} {:?} {}", debug.location(loc), debug.is_distinct_loc(loc)).unwrap();
    }
    writeln!(out, "nodes {}", debug.node_count()).unwrap();
    for unsupported in module.unsupported() {
        writeln!(out, "u {unsupported:?}").unwrap();
    }
    writeln!(out, "symbols {}", module.symbol_count()).unwrap();
    out
}

/// Parses every body of `source` on `threads` threads; the session's
/// part is played by a host that keeps every body.
fn read(source: &[u8], threads: usize) -> (Outcome, Option<ReadStats>) {
    let ParsedModule { mut module, text } =
        parse_module(source, &ParseOptions::default()).expect("the module level parses");
    let mut bodies = Vec::new();
    let mut host = (&mut module, &mut bodies);
    let result = parse_bodies_on(
        &mut host,
        |host| &mut *host.0,
        &text,
        threads,
        |host, function, read| {
            // The module the body is delivered with is the sequential
            // reader's at that point: the body verifies against it.
            if let Ok(body) = &read {
                body.verify(host.0).expect("a delivered body verifies against the module");
            }
            let read =
                read.map_err(|constructs| constructs.iter().map(ToString::to_string).collect());
            host.1.push((function.sym.raw(), read));
            true
        },
    );
    let (stats, error) = match result {
        Ok(stats) => (Some(stats), None),
        Err(error) => (None, Some(error)),
    };
    (Outcome { bodies, error, tables: tables(&module) }, stats)
}

/// Every thread count gives the sequential outcome. Without an error,
/// no body is read again on the module: the forks did all the reading.
fn same_at_every_count(name: &str, source: &[u8]) -> Outcome {
    let (sequential, _) = read(source, 1);
    for threads in THREADS {
        // Twice: the threads take the bodies in another order each time.
        for _ in 0..2 {
            let (parallel, stats) = read(source, threads);
            if let Some(stats) = stats.filter(|_| threads > 1 && parallel.bodies.len() > 1) {
                assert_eq!(stats.threads, threads, "{name}");
                assert_eq!(stats.read_again, 0, "{name} at {threads} threads: {stats:?}");
            }
            if parallel != sequential {
                assert_eq!(parallel.error, sequential.error, "{name} at {threads} threads");
                assert_eq!(parallel.tables, sequential.tables, "{name} at {threads} threads");
                for (at, (got, want)) in parallel.bodies.iter().zip(&sequential.bodies).enumerate()
                {
                    assert!(got == want, "{name} at {threads} threads: body {at} differs");
                }
                panic!(
                    "{name} at {threads} threads: {} bodies, not {}",
                    parallel.bodies.len(),
                    sequential.bodies.len()
                );
            }
        }
    }
    sequential
}

/// A small deterministic generator (xorshift), so that a failure names
/// its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A module whose bodies race for the same new types and constants: each
/// body interns a few of a small set (so that bodies on different
/// threads make the same ones), some of its own, refers forward to
/// values and blocks, and is huge or tiny at random.
fn adversarial(seed: u64, functions: usize) -> String {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut out = String::from(
        "target datalayout = \"e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128\"\n\
         target triple = \"aarch64-unknown-linux-gnu\"\n\n\
         declare void @sink(ptr)\ndeclare i32 @use(...)\n\n",
    );
    for f in 0..functions {
        let size = if rng.below(8) == 0 { 300 + rng.below(1500) } else { rng.below(6) };
        writeln!(out, "define i32 @f{f}(i32 %x) {{").unwrap();
        out.push_str("entry:\n");
        // A forward reference: a block and a value defined below.
        out.push_str("  br label %body\n\nbody:\n");
        out.push_str("  %fwd = phi i32 [ %x, %entry ], [ %last, %body ]\n");
        let mut last = String::from("%x");
        for i in 0..size {
            let shared = rng.below(24);
            let own = f as u64 * 1000 + i;
            let line = match rng.below(9) {
                0 => format!("  %v{i} = add i32 {last}, {shared}\n"),
                1 => format!("  %v{i} = mul i32 {last}, {own}\n"),
                2 => format!(
                    "  %a{i} = alloca [{} x i{}], align 4\n  call void @sink(ptr %a{i})\n  %v{i} = add i32 {last}, 1\n",
                    1 + shared,
                    8 * (1 + shared % 4)
                ),
                3 => format!(
                    "  %a{i} = alloca {{ i32, [{} x i8], <{} x i16> }}, align 8\n  call void @sink(ptr %a{i})\n  %v{i} = sub i32 {last}, {shared}\n",
                    1 + shared,
                    1 + shared % 4
                ),
                4 => format!(
                    "  %w{i} = insertelement <4 x i32> <i32 {shared}, i32 1, i32 {own}, i32 3>, i32 {last}, i32 0\n  %v{i} = extractelement <4 x i32> %w{i}, i32 {}\n",
                    shared % 4
                ),
                5 => format!(
                    "  %w{i} = call i32 (...) @use({{ i32, i64 }} {{ i32 {shared}, i64 {own} }}, [2 x i16] [i16 {shared}, i16 7])\n  %v{i} = xor i32 {last}, %w{i}\n"
                ),
                6 => format!("  %v{i} = select i1 true, i32 {last}, i32 {shared}\n"),
                7 => format!(
                    "  %p{i} = getelementptr inbounds {{ i32, [{} x i64] }}, ptr null, i64 0, i32 1, i64 {}\n  %q{i} = ptrtoint ptr %p{i} to i32\n  %v{i} = add i32 {last}, %q{i}\n",
                    2 + shared,
                    shared % 2
                ),
                _ => format!("  %v{i} = call i32 asm \"mov $0, #{}\", \"=r\"()\n", shared % 5),
            };
            out.push_str(&line);
            last = format!("%v{i}");
        }
        writeln!(out, "  %last = add i32 {last}, 0").unwrap();
        out.push_str("  %done = icmp eq i32 %last, %fwd\n");
        out.push_str("  br i1 %done, label %out, label %body\n\nout:\n");
        out.push_str("  ret i32 %last\n}\n\n");
    }
    out
}

/// The hand-written bodies of `tests/data`.
#[test]
fn handwritten_modules_read_alike_at_every_thread_count() {
    for (name, source) in common::handwritten() {
        if parse_module(&source, &ParseOptions::default()).is_err() {
            continue;
        }
        same_at_every_count(&name, &source);
    }
}

/// Bodies that intern the same new types and constants, forward
/// references, huge and tiny bodies mixed: randomized, seeds named.
#[test]
fn adversarial_orders_read_alike_at_every_thread_count() {
    for seed in 1..=6 {
        let source = adversarial(seed, 40 + seed as usize * 7);
        let outcome = same_at_every_count(&format!("seed {seed}"), source.as_bytes());
        assert!(outcome.error.is_none(), "seed {seed}: {:?}", outcome.error);
        assert!(outcome.bodies.iter().all(|(_, body)| body.is_ok()));
    }
}

/// What clang makes of the C++ sources, with and without `-g`: the
/// debug expressions and locations of bodies, landing pads, switches.
#[test]
fn clang_modules_read_alike_at_every_thread_count() {
    let Some(llvm) = common::Llvm::find(22) else { return };
    for source in ["stl.cpp", "cxx.cpp", "debug_types.cpp", "debug_c.c", "globals.c"] {
        for options in [&[][..], &["-g"][..]] {
            let ir = llvm.clang(source, hod_ir::Arch::Aarch64, options);
            same_at_every_count(&format!("{source} {options:?}"), &ir);
        }
    }
}

/// A malformed body in the middle, and another after it: the error is
/// the first in file order, the sequential parser's, at every count.
#[test]
fn the_first_error_in_file_order_is_returned() {
    for seed in 1..=4 {
        let mut source = adversarial(seed, 60);
        for (victim, junk) in
            [(17, "  %bad = frobnicate i32 1\n"), (41, "  %worse = add i32 %nowhere, 1\n")]
        {
            let at = source.find(&format!("@f{victim}(")).expect("the function");
            let body = at + source[at..].find("body:\n").expect("its body") + "body:\n".len();
            let body = body + source[body..].find('\n').expect("the phi") + 1;
            source.insert_str(body, junk);
        }
        let outcome = same_at_every_count(&format!("seed {seed}"), source.as_bytes());
        let error = outcome.error.expect("an error");
        let at = error.offset as usize;
        assert!(source[at..].starts_with("frobnicate"), "seed {seed}: {error:?}");
        // The bodies before it were delivered, and none after.
        assert_eq!(outcome.bodies.len(), 17, "seed {seed}");
    }
}

/// A body that is wrong only in a way a fork cannot see early (an
/// undefined value in its last instruction) among huge ones.
#[test]
fn a_late_error_in_a_huge_body_is_the_sequential_one() {
    let mut source = adversarial(9, 30);
    let at = source.rfind("  ret i32 %last\n").expect("the last body's ret");
    source.insert_str(at, "  %late = add i32 %undefined_here, 1\n");
    let outcome = same_at_every_count("late", source.as_bytes());
    assert!(outcome.error.is_some());
    assert_eq!(outcome.bodies.len(), 29);
}
