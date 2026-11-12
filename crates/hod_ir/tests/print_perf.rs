//! A measurement, not a gate (`spec/ir.md` §9.9): printing a module of
//! 5,000 small functions, each a loop of loads, stores, arithmetic, a
//! compare, a `phi` and a call, plus as many variables. The throughput
//! is printed (`cargo test --release -p hod_ir --test print_perf --
//! --nocapture`); what the test asserts is only that the text has every
//! function and is the same through an `IoSink`.
#![expect(clippy::many_single_char_names, reason = "short names for the values of a body")]

mod print_support;

use std::time::Instant;

use hod_ir::{
    Body, Dialect, IntPredicate, IoSink, Module, Opcode, TargetSpec, TypeId, VariableDesc,
};
use print_support::{Fx, declare, define, int};

const FUNCTIONS: usize = 5_000;
const I64: TypeId = TypeId::I64;
const PTR: TypeId = TypeId::PTR;

/// The module and its bodies: `f<n>(p, n)` sums `p[i] * i + g<n>` over
/// `i < n`, stores the sum to `v<n>` and calls `sink`.
fn module() -> (Module, Vec<Body>) {
    let mut m = Module::new(TargetSpec::x86_64_linux_gnu());
    let sink = declare(&mut m, "sink", TypeId::VOID, &[I64], false);
    let mut bodies = Vec::with_capacity(FUNCTIONS);
    for n in 0..FUNCTIONS {
        let init = int(&mut m, I64, n as i64);
        let v = m
            .add_variable(
                format!("v{n}").as_bytes(),
                &VariableDesc { init: Some(init), ..VariableDesc::new(I64) },
            )
            .unwrap();
        let f = define(&mut m, &format!("f{n}"), I64, &[PTR, I64]);
        let mut x = Fx::new(&mut m, f);
        let entry = x.here();
        let (head, done) = (x.block(), x.block());
        let zero = x.i64(0);
        x.br(head);
        x.at(head);
        let i = x.phi(I64, &[(zero, entry)]);
        let acc = x.phi(I64, &[(zero, entry)]);
        let slot = x.ptr_add(x.arg(0), 0, &[(i, 8)]);
        let item = x.load(I64, slot);
        let product = x.bin(Opcode::Mul, I64, item, i);
        let global = x.sym(v);
        let bias = x.load(I64, global);
        let sum = x.bin(Opcode::Add, I64, product, bias);
        let acc2 = x.bin(Opcode::Add, I64, acc, sum);
        let one = x.i64(1);
        let i2 = x.bin(Opcode::Add, I64, i, one);
        x.b.add_incoming(i, i2, head).unwrap();
        x.b.add_incoming(acc, acc2, head).unwrap();
        let more = x.icmp(IntPredicate::Slt, i2, x.arg(1));
        x.cond_br(more, head, done);
        x.at(done);
        x.store(acc2, global);
        x.call(sink, &[acc2]);
        x.ret(acc2);
        bodies.push(x.done());
    }
    (m, bodies)
}

/// §9.9: the time to print the module as LLVM 22 text into a `String`
/// and through an `IoSink`, in MB/s.
#[test]
fn print_5k_functions() {
    let (m, bodies) = module();
    let mut text = String::new();
    let start = Instant::now();
    m.print(&mut text, Dialect::LLVM22, &bodies).unwrap();
    let string_time = start.elapsed().as_secs_f64();
    let mut sink = IoSink::new(Vec::with_capacity(text.len()));
    let start = Instant::now();
    m.print(&mut sink, Dialect::LLVM22, &bodies).unwrap();
    let sink_time = start.elapsed().as_secs_f64();
    assert_eq!(sink.finish().unwrap(), text.as_bytes());
    let defines = text.lines().filter(|line| line.starts_with("define ")).count();
    assert_eq!(defines, FUNCTIONS);
    #[expect(clippy::cast_precision_loss, reason = "a measurement")]
    let megabytes = text.len() as f64 / 1e6;
    let instructions: usize = bodies.iter().map(Body::inst_count).sum();
    eprintln!(
        "print_perf: {FUNCTIONS} functions, {instructions} instructions, {megabytes:.2} MB: \
         String {:.1} ms ({:.0} MB/s), IoSink {:.1} ms ({:.0} MB/s)",
        string_time * 1e3,
        megabytes / string_time,
        sink_time * 1e3,
        megabytes / sink_time,
    );
}
