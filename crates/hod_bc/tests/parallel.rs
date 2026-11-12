//! `spec/bc.md` §7, `spec/ir.md` §5.1: bodies read on several threads,
//! against forks of the frozen module, are the sequential reader's
//! bodies, and the module ends as the sequential reader leaves it: every
//! table entry by entry, every id, every unsupported construct, and the
//! first error in file order.

mod common;

use std::fmt::Write as _;
use std::process::Command;

use hod_bc::{BcError, ReadOptions, read_bodies_on, read_module};
use hod_ir::{AsmId, Body, ConstId, ExprId, LocId, Module, ReadStats, TypeId};
use hod_testutil::{DEFAULT_TIMEOUT, run_with};

const THREADS: [usize; 5] = [1, 2, 3, 8, 24];
const LLVM: u32 = 22;

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    bodies: Vec<(u32, Result<Body, Vec<String>>)>,
    error: Option<BcError>,
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

fn read(bitcode: &[u8], threads: usize) -> (Outcome, Option<ReadStats>) {
    let hod_bc::BcModule { mut module, bits } =
        read_module(bitcode, &ReadOptions::default()).expect("the module level reads");
    let mut bodies = Vec::new();
    let mut host = (&mut module, &mut bodies);
    let result = read_bodies_on(
        &mut host,
        |host| &mut *host.0,
        &bits,
        threads,
        |host, function, read| {
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

fn same_at_every_count(name: &str, bitcode: &[u8]) -> Outcome {
    let (sequential, _) = read(bitcode, 1);
    for threads in THREADS {
        for _ in 0..2 {
            let (parallel, stats) = read(bitcode, threads);
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
                panic!("{name} at {threads} threads: the bodies delivered differ in number");
            }
        }
    }
    sequential
}

/// A C++ translation unit with what bodies intern: templates instantiated
/// with the same and with new types, switches, exceptions, inline
/// assembly, and, with `-g`, locations and expressions.
const SOURCE: &str = r#"
#include <cstdint>
struct Pair { int a; long b; };
template <class T, int N> struct Box { T items[N]; T sum() const { T s{}; for (auto &i : items) s += i; return s; } };
template <class T> T twice(T x) { return x + x; }
int thrower(int x) { if (x > 3) throw x; return x; }
int catcher(int x) { try { return thrower(x); } catch (int y) { return y * 2; } catch (...) { return -1; } }
int pick(int x) { switch (x) { case 1: return 10; case 7: return 70; case 100: return 3; default: return x; } }
long asm_one(long x) { long r; asm volatile("mov %0, %1" : "=r"(r) : "r"(x)); return r; }
#define MANY(n) int f##n(int x) { Box<int, n + 1> b{}; Box<Pair *, (n % 5) + 1> p{}; b.items[0] = x; \
    return b.sum() + twice<int>(n) + (int)twice<long>(x) + pick(x + n) + (p.items[0] ? 1 : 0); }
MANY(1) MANY(2) MANY(3) MANY(4) MANY(5) MANY(6) MANY(7) MANY(8) MANY(9) MANY(10)
MANY(11) MANY(12) MANY(13) MANY(14) MANY(15) MANY(16) MANY(17) MANY(18) MANY(19) MANY(20)
double fp(double d) { Box<double, 3> b{{d, d * 2, d * 3}}; return b.sum() + twice(d); }
"#;

/// What clang makes of [`SOURCE`] as bitcode; `None` without clang.
fn clang(options: &[&str]) -> Option<Vec<u8>> {
    let bin = common::llvm_bin(LLVM)?;
    let clang = bin.join("clang");
    if !clang.exists() {
        return None;
    }
    let mut command = Command::new(clang);
    command.args(["--target=aarch64-linux-gnu", "-x", "c++", "-std=c++20", "-O0", "-w"]);
    command.args(["-c", "-emit-llvm", "-o", "-", "-"]).args(options);
    let out = run_with(&mut command, SOURCE.as_bytes(), DEFAULT_TIMEOUT);
    assert!(out.success(), "clang: {}", out.stderr_text());
    Some(out.stdout)
}

#[test]
fn clang_bitcode_reads_alike_at_every_thread_count() {
    for options in [&[][..], &["-g"][..]] {
        let Some(bitcode) = clang(options) else { return };
        let outcome = same_at_every_count(&format!("{options:?}"), &bitcode);
        assert!(outcome.error.is_none(), "{options:?}: {:?}", outcome.error);
        assert!(outcome.bodies.len() > 40, "{options:?}: {} bodies", outcome.bodies.len());
    }
}

/// The checked-in bitcode files.
#[test]
fn checked_in_bitcode_reads_alike_at_every_thread_count() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    for name in ["bodies-llvm19.bc", "debug-llvm22.bc", "module-llvm19.bc"] {
        let bitcode = std::fs::read(dir.join(name)).expect("a test file");
        if read_module(&bitcode, &ReadOptions::default()).is_err() {
            continue;
        }
        same_at_every_count(name, &bitcode);
    }
}

/// Bodies broken in the middle of the file: the error is the first in
/// file order and the sequential reader's at every count, and no body
/// after it is delivered.
#[test]
fn the_first_error_in_file_order_is_returned() {
    let Some(text) = common::llvm_bin(LLVM).map(|_| {
        let mut out = String::from(
            "target datalayout = \"e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128\"\n\
             target triple = \"aarch64-unknown-linux-gnu\"\n\ndeclare i32 @use(...)\n\n",
        );
        for f in 0..40 {
            writeln!(
                out,
                "define i32 @f{f}(i32 %x) {{\n  %a = add i32 %x, {f}\n  \
                 %b = call i32 (...) @use({{ i32, [{} x i8] }} zeroinitializer)\n  \
                 %c = udiv i32 %a, %b\n  ret i32 %c\n}}\n",
                f % 7 + 1
            )
            .unwrap();
        }
        out
    }) else {
        return;
    };
    let Ok(bitcode) = common::assemble(LLVM, text.as_bytes()) else { return };
    let spans: Vec<(u64, u64)> = {
        let read = read_module(&bitcode, &ReadOptions::default()).expect("reads");
        read.bits.functions().iter().map(|f| (f.span.start, f.span.end)).collect()
    };
    // Bytes flipped inside the bodies of functions 13 and 29: what the
    // sequential reader makes of that, an error or not, is what every
    // thread count makes of it.
    let mut errors = 0;
    for shift in 0..24u64 {
        let mut broken = bitcode.clone();
        for victim in [13, 29] {
            let (start, end) = spans[victim];
            let byte = (start / 8 + 1 + (shift * 7) % ((end - start) / 8 - 2)) as usize;
            broken[byte] ^= 0x5a;
        }
        if read_module(&broken, &ReadOptions::default()).is_err() {
            continue;
        }
        let outcome = same_at_every_count(&format!("shift {shift}"), &broken);
        if outcome.error.is_some() {
            errors += 1;
            // No body after the error is delivered.
            assert!(outcome.bodies.len() <= 29, "shift {shift}");
        }
    }
    assert!(errors > 0, "no corruption was an error");
}
