//! `spec/ir.md` §9.4 and §9.6: printed bodies behave under `llc` as the IR
//! says. Programs are written through `FunctionBuilder`, printed, compiled
//! by `llc` at `-O0` and at `-O2`, linked by `gcc` and run; what they print
//! and return is compared with the same arithmetic done here.
//!
//! The programs use loops and `phi`s (with values reserved before their
//! definitions), `switch`, direct, indirect and variadic calls, calling
//! conventions and parameter attributes, stack slots, dynamic `alloca`s,
//! `PtrAdd` with negative, narrow, wide and scaled indices, aggregates by
//! byte offset, `i128`, floats, atomics, vectors, `indirectbr` over
//! `blockaddress` constants, intrinsics with overload suffixes, a
//! thread-local variable, anonymous symbols, variables whose initializers
//! need relocations, and a C++ exception thrown and caught.
//!
//! The same program split into two texts (`Part::Main`, `Part::Rest`)
//! prints the same and returns the same.
//!
//! They run only where `llc`'s target is the host: AArch64 Linux.

mod print_support;

use std::fmt::Write as _;

use hod_ir::{
    AtomicOrdering, BlockId, Body, CallConv, CallDesc, Clause, Dialect, FloatPredicate,
    FunctionDesc, IntPredicate, Intrinsic, Linkage, MemFlags, Module, Opcode, ParamAttrs,
    ParamMem, Part, PicLevel, SymId, SyncScope, TailKind, TargetSpec, TlsModel, TypeId, Value,
    VariableDesc,
};
use hod_testutil::Scratch;
use print_support::{Fx, Llvm, declare, define, int, link_and_run, printed};

const I1: TypeId = TypeId::I1;
const I8: TypeId = TypeId::I8;
const I32: TypeId = TypeId::I32;
const I64: TypeId = TypeId::I64;
const PTR: TypeId = TypeId::PTR;
const DOUBLE: TypeId = TypeId::DOUBLE;

/// Whether the programs can run here.
fn host_is_aarch64_linux() -> bool {
    let host = cfg!(all(target_arch = "aarch64", target_os = "linux"));
    if !host {
        eprintln!("note: skipping: the printed programs run on AArch64 Linux only");
    }
    host
}

fn new_module() -> Module {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    m.props.pic = PicLevel::Big;
    m
}

/// A program being written: its module, its bodies, and what it is to
/// print.
struct Program {
    m: Module,
    bodies: Vec<Body>,
    expected: String,
    /// The exclusive or of everything printed.
    check: i64,
}

impl Program {
    fn body(&mut self, name: &str, ret: TypeId, params: &[TypeId]) -> Fx<'_> {
        let f = define(&mut self.m, name, ret, params);
        Fx::new(&mut self.m, f)
    }

    fn internal_variable(&mut self, name: &str, init: hod_ir::ConstId, constant: bool) -> SymId {
        let ty = self.m.consts.ty(init);
        let desc = VariableDesc {
            linkage: Linkage::Internal,
            constant,
            init: Some(init),
            ..VariableDesc::new(ty)
        };
        self.m.add_variable(name.as_bytes(), &desc).unwrap()
    }

    fn function(&self, name: &str) -> SymId {
        self.m.find_symbol(name.as_bytes()).unwrap_or_else(|| panic!("no function {name}"))
    }
}

// ---- the functions, each with the arithmetic it stands for ----

/// `sum_squares(n)`: the squares of 0 to n-1, summed in a loop whose
/// `phi`s get their second incoming value after it is defined.
fn sum_squares(p: &mut Program) {
    let mut x = p.body("sum_squares", I64, &[I64]);
    let entry = x.here();
    let (body, exit) = (x.block(), x.block());
    x.br(body);
    x.at(body);
    let zero = x.i64(0);
    let i = x.phi(I64, &[(zero, entry)]);
    let acc = x.phi(I64, &[(zero, entry)]);
    let square = x.bin(Opcode::Mul, I64, i, i);
    let acc2 = x.bin(Opcode::Add, I64, acc, square);
    let one = x.i64(1);
    let i2 = x.bin(Opcode::Add, I64, i, one);
    x.b.add_incoming(i, i2, body).unwrap();
    x.b.add_incoming(acc, acc2, body).unwrap();
    let more = x.icmp(IntPredicate::Slt, i2, x.arg(0));
    x.cond_br(more, body, exit);
    x.at(exit);
    x.ret(acc2);
    let body = x.done();
    p.bodies.push(body);
}

/// `factorial(n)`: a loop whose `phi`s name values that are reserved
/// first and defined later (§5.3).
fn factorial(p: &mut Program) {
    let mut x = p.body("factorial", I64, &[I64]);
    let entry = x.here();
    let (body, exit) = (x.block(), x.block());
    x.br(body);
    x.at(body);
    let one = x.i64(1);
    let next_i = x.b.reserve(I64).unwrap();
    let next_product = x.b.reserve(I64).unwrap();
    let i = x.phi(I64, &[(one, entry), (next_i, body)]);
    let product = x.phi(I64, &[(one, entry), (next_product, body)]);
    let product2 = x.bin(Opcode::Mul, I64, product, i);
    let i2 = x.bin(Opcode::Add, I64, i, one);
    x.b.resolve(next_i, i2).unwrap();
    x.b.resolve(next_product, product2).unwrap();
    let more = x.icmp(IntPredicate::Ule, i2, x.arg(0));
    x.cond_br(more, body, exit);
    x.at(exit);
    x.ret(product2);
    let body = x.done();
    p.bodies.push(body);
}

/// `classify(x)`: a `switch` with a negative case and two cases that
/// share a block.
fn classify(p: &mut Program) {
    let mut x = p.body("classify", I64, &[I32]);
    let (a, b, c, d, merge) = (x.block(), x.block(), x.block(), x.block(), x.block());
    let cases = [
        (int(x.m, I32, 0), a),
        (int(x.m, I32, 1), b),
        (int(x.m, I32, 7), a),
        (int(x.m, I32, -3), c),
    ];
    x.b.switch(x.arg(0), d, &cases).unwrap();
    for block in [a, b, c, d] {
        x.at(block);
        x.br(merge);
    }
    x.at(merge);
    let values = [(x.i64(10), a), (x.i64(20), b), (x.i64(30), c), (x.i64(40), d)];
    let result = x.phi(I64, &values);
    x.ret(result);
    let body = x.done();
    p.bodies.push(body);
}

fn classify_model(x: i32) -> i64 {
    match x {
        0 | 7 => 10,
        1 => 20,
        -3 => 30,
        _ => 40,
    }
}

/// `apply(which, x)`: an indirect call through a table of internal
/// functions, a constant whose initializer needs relocations; and
/// `middle()`, a load through a pointer variable initialized with an
/// address plus an offset.
fn apply(p: &mut Program) {
    let mut table = Vec::new();
    for (name, op) in [("inc", Opcode::Add), ("dbl", Opcode::Mul), ("neg", Opcode::Sub)] {
        let ty = p.m.types.function(I64, &[I64], false).unwrap();
        let desc =
            FunctionDesc { linkage: Linkage::Internal, has_body: true, ..FunctionDesc::new(ty) };
        let f = p.m.add_function(name.as_bytes(), &desc).unwrap();
        let mut x = Fx::new(&mut p.m, f);
        let result = match op {
            Opcode::Add => {
                let one = x.i64(1);
                x.bin(op, I64, x.arg(0), one)
            }
            Opcode::Mul => {
                let two = x.i64(2);
                x.bin(op, I64, x.arg(0), two)
            }
            _ => {
                let zero = x.i64(0);
                x.bin(op, I64, zero, x.arg(0))
            }
        };
        x.ret(result);
        let body = x.done();
        p.bodies.push(body);
        table.push(p.m.consts.addr(f, 0).unwrap());
    }
    let table_ty = p.m.types.array(PTR, 3).unwrap();
    let init = p.m.consts.aggregate(&p.m.types, table_ty, &table).unwrap();
    let fns = p.internal_variable("fns", init, true);

    let fn_ty = p.m.types.function(I64, &[I64], false).unwrap();
    let mut x = p.body("apply", I64, &[I64, I64]);
    let base = x.sym(fns);
    let entry = x.ptr_add(base, 0, &[(x.arg(0), 8)]);
    let callee = x.load(PTR, entry);
    let result = x.b.call(I64, &CallDesc::new(callee, fn_ty, &[x.arg(1)])).unwrap();
    x.ret(result);
    let body = x.done();
    p.bodies.push(body);

    let words_ty = p.m.types.array(I32, 4).unwrap();
    let words: Vec<_> = [11, 22, 33, 44].iter().map(|&word| int(&mut p.m, I32, word)).collect();
    let init = p.m.consts.aggregate(&p.m.types, words_ty, &words).unwrap();
    let words = p.internal_variable("words", init, false);
    let third = p.m.consts.addr(words, 8).unwrap();
    let mid = p.internal_variable("mid", third, false);
    let mut x = p.body("middle", I64, &[]);
    let pointer = x.load(PTR, x.sym(mid));
    let word = x.load(I32, pointer);
    let result = x.cast(Opcode::SExt, I64, word);
    x.ret(result);
    let body = x.done();
    p.bodies.push(body);
}

/// `slots(x)`: two stack slots written and read back through constant
/// offsets.
fn slots(p: &mut Program) {
    let mut x = p.body("slots", I64, &[I64]);
    let wide = x.b.add_slot(32, 8).unwrap();
    let narrow = x.b.add_slot(3, 1).unwrap();
    let (wide, narrow) = (x.b.slot(wide).unwrap(), x.b.slot(narrow).unwrap());
    let arg = x.arg(0);
    let (one, two, four) = (x.i64(1), x.i64(2), x.i64(4));
    let values = [
        arg,
        x.bin(Opcode::Add, I64, arg, one),
        x.bin(Opcode::Mul, I64, arg, two),
        x.bin(Opcode::Sub, I64, arg, four),
    ];
    for (at, value) in values.into_iter().enumerate() {
        let place = x.ptr_add(wide, 8 * at as i64, &[]);
        x.store(value, place);
    }
    let low = x.cast(Opcode::Trunc, I8, arg);
    x.store(low, narrow);
    let last = x.ptr_add(narrow, 2, &[]);
    let seven = x.int(I8, 7);
    x.store(seven, last);
    let mut sum = x.i64(0);
    for at in 0..4 {
        let place = x.ptr_add(wide, 8 * at, &[]);
        let value = x.load(I64, place);
        sum = x.bin(Opcode::Add, I64, sum, value);
    }
    for place in [narrow, last] {
        let byte = x.load(I8, place);
        let value = x.cast(Opcode::ZExt, I64, byte);
        sum = x.bin(Opcode::Add, I64, sum, value);
    }
    x.ret(sum);
    let body = x.done();
    p.bodies.push(body);
}

fn slots_model(x: i64) -> i64 {
    x + (x + 1) + x * 2 + (x - 4) + (x & 0xff) + 7
}

/// `ptradd(a, b, c, d)`: loads from `arr` (the numbers 100 to 107) at
/// addresses computed from the middle of the array with an `i8`, an
/// `i16`, an `i32`, an `i64` and an `i128` index, negative ones among
/// them, and scales that are not the element size.
fn ptradd(p: &mut Program) {
    let arr_ty = p.m.types.array(I32, 8).unwrap();
    let elems: Vec<_> = (100..108).map(|value| int(&mut p.m, I32, value)).collect();
    let init = p.m.consts.aggregate(&p.m.types, arr_ty, &elems).unwrap();
    let arr = p.internal_variable("arr", init, false);
    let i16_ty = p.m.types.int(16).unwrap();
    let i128_ty = p.m.types.int(128).unwrap();
    let mut x = p.body("ptradd", I64, &[I8, i16_ty, I32, I64]);
    let base = x.addr(arr, 16);
    let (a, b, c, d) = (x.arg(0), x.arg(1), x.arg(2), x.arg(3));
    let wide = x.cast(Opcode::ZExt, i128_ty, d);
    // With a = -2, b = 3, c = -1, d = 1: the elements 2, 7, 3, 4 and 5.
    let places = [
        x.ptr_add(base, 0, &[(a, 4)]),
        x.ptr_add(base, 0, &[(b, 4)]),
        x.ptr_add(base, 4, &[(c, 8)]),
        x.ptr_add(base, -8, &[(d, 12), (a, 2)]),
        x.ptr_add(base, 0, &[(wide, 4)]),
    ];
    let mut sum = x.i64(0);
    for (place, weight) in places.into_iter().zip([1, 3, 5, 7, 11]) {
        let word = x.load(I32, place);
        let value = x.cast(Opcode::SExt, I64, word);
        let weight = x.i64(weight);
        let scaled = x.bin(Opcode::Mul, I64, value, weight);
        sum = x.bin(Opcode::Add, I64, sum, scaled);
    }
    x.ret(sum);
    let body = x.done();
    p.bodies.push(body);
}

fn ptradd_model(a: i8, b: i16, c: i32, d: i64) -> i64 {
    let arr: Vec<i64> = (100..108).collect();
    let at = |bytes: i64| arr[usize::try_from((16 + bytes) / 4).unwrap()];
    let (a, b, c) = (i64::from(a), i64::from(b), i64::from(c));
    at(a * 4) + 3 * at(b * 4) + 5 * at(4 + c * 8) + 7 * at(-8 + d * 12 + a * 2) + 11 * at(d * 4)
}

/// `make(x)` returns a `{ i8, i32, [2 x i64] }` built with `insertvalue`;
/// `aggregate(x)` takes it apart with `extractvalue`, the inner array as
/// a whole too.
fn aggregate(p: &mut Program) {
    let pair = p.m.types.array(I64, 2).unwrap();
    let record = p.m.types.struct_type(&[I8, I32, pair], false).unwrap();
    assert_eq!(p.m.types.field_offsets(record).unwrap(), [0, 4, 8]);
    let mut x = p.body("make", record, &[I64]);
    let arg = x.arg(0);
    let (three, five) = (x.i64(3), x.i64(5));
    let low = x.cast(Opcode::Trunc, I8, arg);
    let tripled = x.bin(Opcode::Mul, I64, arg, three);
    let word = x.cast(Opcode::Trunc, I32, tripled);
    let more = x.bin(Opcode::Add, I64, arg, five);
    let undef = x.m.consts.undef(&x.m.types, record).unwrap();
    let mut value = x.konst(undef);
    for (field, offset) in [(low, 0), (word, 4), (arg, 8), (more, 16)] {
        value = x.b.insert_value(record, value, field, offset).unwrap();
    }
    x.ret(value);
    let body = x.done();
    p.bodies.push(body);

    let make = p.function("make");
    let mut x = p.body("aggregate", I64, &[I64]);
    let made = x.call(make, &[x.arg(0)]);
    let low = x.b.extract_value(I8, made, 0).unwrap();
    let word = x.b.extract_value(I32, made, 4).unwrap();
    let first = x.b.extract_value(I64, made, 8).unwrap();
    let second = x.b.extract_value(I64, made, 16).unwrap();
    let inner = x.b.extract_value(pair, made, 8).unwrap();
    let again = x.b.extract_value(I64, inner, 8).unwrap();
    let low = x.cast(Opcode::SExt, I64, low);
    let word = x.cast(Opcode::ZExt, I64, word);
    let mut sum = low;
    for value in [word, first, second, again] {
        sum = x.bin(Opcode::Add, I64, sum, value);
    }
    x.ret(sum);
    let body = x.done();
    p.bodies.push(body);
}

fn aggregate_model(x: i64) -> i64 {
    i64::from(x as i8) + i64::from((x * 3) as u32) + x + (x + 5) + (x + 5)
}

/// `wide(a, b)`: a 64 by 64 to 128 bit product, a 128-bit constant, and
/// shifts.
fn wide(p: &mut Program) {
    let i128_ty = p.m.types.int(128).unwrap();
    let big = p.m.consts.int_from_words(&p.m.types, i128_ty, &[7, 1 << 36]).unwrap();
    let mut x = p.body("wide", I64, &[I64, I64]);
    let a = x.cast(Opcode::ZExt, i128_ty, x.arg(0));
    let b = x.cast(Opcode::ZExt, i128_ty, x.arg(1));
    let product = x.bin(Opcode::Mul, i128_ty, a, b);
    let sixty_four = x.int(i128_ty, 64);
    let high = x.bin(Opcode::LShr, i128_ty, product, sixty_four);
    let high = x.cast(Opcode::Trunc, I64, high);
    let low = x.cast(Opcode::Trunc, I64, product);
    let mixed = x.bin(Opcode::Xor, I64, high, low);
    let sum = x.bin(Opcode::Add, i128_ty, product, x.konst(big));
    let hundred = x.int(i128_ty, 100);
    let top = x.bin(Opcode::LShr, i128_ty, sum, hundred);
    let top = x.cast(Opcode::Trunc, I64, top);
    let result = x.bin(Opcode::Add, I64, mixed, top);
    x.ret(result);
    let body = x.done();
    p.bodies.push(body);
}

fn wide_model(a: u64, b: u64) -> i64 {
    let product = u128::from(a) * u128::from(b);
    let mixed = ((product >> 64) as u64) ^ (product as u64);
    let sum = product.wrapping_add((1 << 100) + 7);
    mixed.wrapping_add((sum >> 100) as u64) as i64
}

/// `floats(x)`: double and float arithmetic with constants that are not
/// exact in decimal, a comparison and conversions; the result is the bit
/// pattern.
fn floats(p: &mut Program) {
    let mut x = p.body("floats", I64, &[DOUBLE]);
    let (c1, c2, c3) = (x.f64(1.5), x.f64(0.1), x.f64(3.0));
    let scaled = x.bin(Opcode::FMul, DOUBLE, x.arg(0), c1);
    let moved = x.bin(Opcode::FAdd, DOUBLE, scaled, c2);
    let single = x.cast(Opcode::FPTrunc, TypeId::FLOAT, moved);
    let tenth = x.f32(0.1);
    let single = x.bin(Opcode::FAdd, TypeId::FLOAT, single, tenth);
    let back = x.cast(Opcode::FPExt, DOUBLE, single);
    let third = x.bin(Opcode::FDiv, DOUBLE, back, c3);
    let negated = x.b.fneg(DOUBLE, third).unwrap();
    let bits = x.cast(Opcode::BitCast, I64, negated);
    let limit = x.f64(1e9);
    let small = x.fcmp(FloatPredicate::Olt, third, limit);
    let zero = x.i64(0);
    let kept = x.select(I64, small, bits, zero);
    let thousand = x.f64(1000.0);
    let milli = x.bin(Opcode::FMul, DOUBLE, third, thousand);
    let whole = x.cast(Opcode::FPToSI, I64, milli);
    let converted = x.cast(Opcode::SIToFP, DOUBLE, whole);
    let exact = x.fcmp(FloatPredicate::Une, converted, milli);
    let exact = x.cast(Opcode::ZExt, I64, exact);
    let result = x.bin(Opcode::Xor, I64, kept, whole);
    let result = x.bin(Opcode::Add, I64, result, exact);
    x.ret(result);
    let body = x.done();
    p.bodies.push(body);
}

fn floats_model(x: f64) -> i64 {
    let moved = x * 1.5 + 0.1;
    let single = moved as f32 + 0.1f32;
    let third = f64::from(single) / 3.0;
    let kept = if third < 1e9 { (-third).to_bits() as i64 } else { 0 };
    let milli = third * 1000.0;
    let whole = milli as i64;
    (kept ^ whole) + i64::from(whole as f64 != milli)
}

/// `dynamic(n)`: two dynamic stack allocations, of `n` eight-byte
/// elements and of `n` bytes.
fn dynamic(p: &mut Program) {
    let mut x = p.body("dynamic", I64, &[I64]);
    let n = x.arg(0);
    let words = x.b.alloca(PTR, n, 8, 16).unwrap();
    let bytes = x.b.alloca(PTR, n, 1, 1).unwrap();
    x.store(n, words);
    let two = x.i64(2);
    let doubled = x.bin(Opcode::Mul, I64, n, two);
    let last = x.ptr_add(words, -8, &[(n, 8)]);
    x.store(doubled, last);
    let low = x.cast(Opcode::Trunc, I8, n);
    let last_byte = x.ptr_add(bytes, -1, &[(n, 1)]);
    x.store(low, last_byte);
    let first = x.load(I64, words);
    let second = x.load(I64, last);
    let byte = x.load(I8, last_byte);
    let byte = x.cast(Opcode::ZExt, I64, byte);
    let sum = x.bin(Opcode::Add, I64, first, second);
    let sum = x.bin(Opcode::Add, I64, sum, byte);
    x.ret(sum);
    let body = x.done();
    p.bodies.push(body);
}

/// `atomics()`: `atomicrmw`, `cmpxchg` with its pair result, an atomic
/// load and a `fence`.
fn atomics(p: &mut Program) {
    let pair = p.m.types.struct_type(&[I64, I1], false).unwrap();
    let mut x = p.body("atomics", I64, &[]);
    let slot = x.b.add_slot(8, 8).unwrap();
    let slot = x.b.slot(slot).unwrap();
    let atomic = MemFlags::aligned(8).unwrap().with_atomic(AtomicOrdering::SeqCst, SyncScope::System);
    let (five, seven, twelve, ninety_nine) = (x.i64(5), x.i64(7), x.i64(12), x.i64(99));
    x.b.store(five, slot, atomic).unwrap();
    let old = x.b.atomic_rmw(hod_ir::RmwOp::Add, I64, slot, seven, atomic).unwrap();
    let swapped = x
        .b
        .cmpxchg(pair, slot, twelve, ninety_nine, atomic, AtomicOrdering::SeqCst, false)
        .unwrap();
    let seen = x.b.extract_value(I64, swapped, 0).unwrap();
    let done = x.b.extract_value(I1, swapped, 8).unwrap();
    x.b.fence(AtomicOrdering::SeqCst, SyncScope::System).unwrap();
    let now = x.b.load(I64, slot, atomic).unwrap();
    let done = x.cast(Opcode::ZExt, I64, done);
    let mut sum = old;
    for (value, weight) in [(seen, 10), (done, 1000), (now, 100_000)] {
        let weight = x.i64(weight);
        let scaled = x.bin(Opcode::Mul, I64, value, weight);
        sum = x.bin(Opcode::Add, I64, sum, scaled);
    }
    x.ret(sum);
    let body = x.done();
    p.bodies.push(body);
}

/// `vectors(x)`: `insertelement`, `shufflevector` with a constant vector,
/// a vector addition and `extractelement`.
fn vectors(p: &mut Program) {
    let v4 = p.m.types.vector(I32, 4).unwrap();
    let elems: Vec<_> = (1..=4).map(|value| int(&mut p.m, I32, value)).collect();
    let numbers = p.m.consts.aggregate(&p.m.types, v4, &elems).unwrap();
    let zero = p.m.consts.zero(&p.m.types, v4).unwrap();
    let mut x = p.body("vectors", I64, &[I32]);
    let arg = x.arg(0);
    let two = x.i32(2);
    let doubled = x.bin(Opcode::Mul, I32, arg, two);
    let (at0, at3) = (x.i64(0), x.i32(3));
    let first = x.b.insert_element(v4, x.konst(zero), arg, at0).unwrap();
    let second = x.b.insert_element(v4, first, doubled, at3).unwrap();
    // <x, 1, 2x, 4>
    let mixed = x.b.shuffle_vector(v4, second, x.konst(numbers), &[0, 4, 3, 7]).unwrap();
    let sum = x.bin(Opcode::Add, v4, mixed, mixed);
    let mut total = x.i64(0);
    for lane in 0..4 {
        let index = x.i32(lane);
        let lane = x.b.extract_element(I32, sum, index).unwrap();
        let lane = x.cast(Opcode::ZExt, I64, lane);
        total = x.bin(Opcode::Add, I64, total, lane);
    }
    x.ret(total);
    let body = x.done();
    p.bodies.push(body);
}

/// `dispatch(which)`: an `indirectbr` to an address loaded from a table
/// of `blockaddress` constants.
fn dispatch(p: &mut Program) {
    let f = define(&mut p.m, "dispatch", I64, &[I64]);
    // The blocks the body is about to create.
    let labels: Vec<_> =
        [1, 2].iter().map(|&block| p.m.consts.block_addr(f, BlockId::new(block)).unwrap()).collect();
    let table_ty = p.m.types.array(PTR, 2).unwrap();
    let init = p.m.consts.aggregate(&p.m.types, table_ty, &labels).unwrap();
    let table = p.internal_variable("labels", init, true);
    let mut x = Fx::new(&mut p.m, f);
    let (first, second) = (x.block(), x.block());
    assert_eq!((first, second), (BlockId::new(1), BlockId::new(2)));
    let base = x.sym(table);
    let entry = x.ptr_add(base, 0, &[(x.arg(0), 8)]);
    let target = x.load(PTR, entry);
    x.b.indirect_br(target, &[first, second]).unwrap();
    for (block, value) in [(first, 111), (second, 222)] {
        x.at(block);
        let value = x.i64(value);
        x.ret(value);
    }
    let body = x.done();
    p.bodies.push(body);
}

/// `intrinsics(x, y)`: intrinsics whose names carry their overload types,
/// one of them with a pair result, and a `memcpy` from a string constant.
fn intrinsics(p: &mut Program) {
    let text = p.m.consts.bytes(&mut p.m.types, b"ABCDEFGH").unwrap();
    let text = p.internal_variable("text", text, true);
    let pair = p.m.types.struct_type(&[I32, I1], false).unwrap();
    let mut x = p.body("intrinsics", I64, &[I32, I64]);
    let slot = x.b.add_slot(8, 8).unwrap();
    let slot = x.b.slot(slot).unwrap();
    let (arg, other) = (x.arg(0), x.arg(1));
    let ones = x.intrinsic(Intrinsic::Ctpop, &[I32], I32, &[I32], &[arg]);
    let fifty = x.i64(50);
    let larger = x.intrinsic(Intrinsic::Smax, &[I64], I64, &[I64, I64], &[other, fifty]);
    let most = x.i32(i64::from(i32::MAX));
    let added = x.intrinsic(Intrinsic::SaddWithOverflow, &[I32], pair, &[I32, I32], &[arg, most]);
    let overflowed = x.b.extract_value(I1, added, 4).unwrap();
    let (eight, no) = (x.i64(8), x.int(I1, 0));
    let from = x.sym(text);
    x.intrinsic(
        Intrinsic::Memcpy,
        &[PTR, PTR, I64],
        TypeId::VOID,
        &[PTR, PTR, I64, I1],
        &[slot, from, eight, no],
    );
    let copied = x.load(I64, slot);
    let swapped = x.intrinsic(Intrinsic::Bswap, &[I64], I64, &[I64], &[copied]);
    let ones = x.cast(Opcode::ZExt, I64, ones);
    let overflowed = x.cast(Opcode::ZExt, I64, overflowed);
    let thousand = x.i64(1000);
    let overflowed = x.bin(Opcode::Mul, I64, overflowed, thousand);
    let mut sum = swapped;
    for value in [ones, larger, overflowed] {
        sum = x.bin(Opcode::Add, I64, sum, value);
    }
    x.ret(sum);
    let body = x.done();
    p.bodies.push(body);
}

fn intrinsics_model(x: i32, y: i64) -> i64 {
    let copied = u64::from_le_bytes(*b"ABCDEFGH");
    copied.swap_bytes() as i64
        + i64::from(x.count_ones())
        + y.max(50)
        + 1000 * i64::from(x.checked_add(i32::MAX).is_none())
}

/// `counter()`: a thread-local variable, reached through
/// `llvm.threadlocal.address`, incremented and returned.
fn counter(p: &mut Program) {
    let init = int(&mut p.m, I64, 17);
    let desc = VariableDesc {
        tls: TlsModel::GeneralDynamic,
        init: Some(init),
        ..VariableDesc::new(I64)
    };
    let tl = p.m.add_variable(b"tl", &desc).unwrap();
    let mut x = p.body("counter", I64, &[]);
    let place = x.intrinsic(Intrinsic::ThreadLocalAddress, &[PTR], PTR, &[PTR], &[x.sym(tl)]);
    let old = x.load(I64, place);
    let one = x.i64(1);
    let new = x.bin(Opcode::Add, I64, old, one);
    x.store(new, place);
    x.ret(new);
    let body = x.done();
    p.bodies.push(body);
}

/// `misc(x)`: extensions, truncation, shifts, division, `freeze`,
/// `select` and a pointer that survives `ptrtoint` and `inttoptr`.
fn misc(p: &mut Program) {
    let mut x = p.body("misc", I64, &[I32]);
    let slot = x.b.add_slot(4, 4).unwrap();
    let slot = x.b.slot(slot).unwrap();
    let arg = x.b.freeze(I32, x.arg(0)).unwrap();
    let signed = x.cast(Opcode::SExt, I64, arg);
    let unsigned = x.cast(Opcode::ZExt, I64, arg);
    let byte = x.cast(Opcode::Trunc, I8, arg);
    let byte = x.cast(Opcode::SExt, I64, byte);
    let (three, five, seven) = (x.i64(3), x.i64(5), x.i64(7));
    let shifted = x.bin(Opcode::AShr, I64, signed, three);
    let logical = x.bin(Opcode::LShr, I64, unsigned, five);
    let remainder = x.bin(Opcode::SRem, I64, signed, seven);
    let quotient = x.bin(Opcode::UDiv, I64, unsigned, three);
    let zero = x.i32(0);
    let negative = x.icmp(IntPredicate::Slt, arg, zero);
    let chosen = x.select(I64, negative, signed, unsigned);
    let number = x.cast(Opcode::PtrToInt, I64, slot);
    let pointer = x.cast(Opcode::IntToPtr, PTR, number);
    let same = x.icmp(IntPredicate::Eq, pointer, slot);
    let same = x.cast(Opcode::ZExt, I64, same);
    let mut result = signed;
    for value in [unsigned, byte, shifted, logical, remainder, quotient, chosen, same] {
        result = x.bin(Opcode::Xor, I64, result, value);
    }
    x.ret(result);
    let body = x.done();
    p.bodies.push(body);
}

fn misc_model(x: i32) -> i64 {
    let (signed, unsigned) = (i64::from(x), i64::from(x as u32));
    let chosen = if x < 0 { signed } else { unsigned };
    signed
        ^ unsigned
        ^ i64::from(x as i8)
        ^ (signed >> 3)
        ^ (unsigned >> 5)
        ^ (signed % 7)
        ^ (unsigned / 3)
        ^ chosen
        ^ 1
}

/// `anonymous(x)`: a call of a function without a name, which reads a
/// variable without a name.
fn anonymous(p: &mut Program) {
    let five = int(&mut p.m, I64, 5);
    let variable = p.m.anonymous_symbol().unwrap();
    let desc = VariableDesc { linkage: Linkage::Private, init: Some(five), ..VariableDesc::new(I64) };
    p.m.set_variable(variable, &desc).unwrap();
    let ty = p.m.types.function(I64, &[I64], false).unwrap();
    let function = p.m.anonymous_symbol().unwrap();
    let desc = FunctionDesc { linkage: Linkage::Internal, has_body: true, ..FunctionDesc::new(ty) };
    p.m.set_function(function, &desc).unwrap();
    let mut x = Fx::new(&mut p.m, function);
    let three = x.i64(3);
    let tripled = x.bin(Opcode::Mul, I64, x.arg(0), three);
    let added = x.load(I64, x.sym(variable));
    let result = x.bin(Opcode::Add, I64, tripled, added);
    x.ret(result);
    let body = x.done();
    p.bodies.push(body);

    let mut x = p.body("anonymous", I64, &[I64]);
    let result = x.call(function, &[x.arg(0)]);
    x.ret(result);
    let body = x.done();
    p.bodies.push(body);
}

/// `conventions(x)`: calls with parameter attributes (`signext`,
/// `zeroext`, `byval`, `sret`), a calling convention and a tail marker.
fn conventions(p: &mut Program) {
    let i16_ty = p.m.types.int(16).unwrap();
    let pair = p.m.types.struct_type(&[I64, I64], false).unwrap();
    let signext = ParamAttrs::NONE.with(ParamAttrs::SIGNEXT).unwrap();
    let zeroext = ParamAttrs::NONE.with(ParamAttrs::ZEROEXT).unwrap();
    let byval = ParamAttrs::NONE.with_mem(ParamMem::ByVal, pair).unwrap().with_align(8).unwrap();
    let sret = ParamAttrs::NONE.with_mem(ParamMem::Sret, pair).unwrap().with_align(8).unwrap();

    // narrow(i8 signext, i16 zeroext) -> i32 signext
    let narrow_ty = p.m.types.function(I32, &[I8, i16_ty], false).unwrap();
    let desc = FunctionDesc {
        ret_attrs: signext,
        param_attrs: &[signext, zeroext],
        has_body: true,
        ..FunctionDesc::new(narrow_ty)
    };
    let narrow = p.m.add_function(b"narrow", &desc).unwrap();
    let mut x = Fx::new(&mut p.m, narrow);
    let a = x.cast(Opcode::SExt, I32, x.arg(0));
    let b = x.cast(Opcode::ZExt, I32, x.arg(1));
    let sum = x.bin(Opcode::Add, I32, a, b);
    x.ret(sum);
    let body = x.done();
    p.bodies.push(body);

    // take(ptr byval({ i64, i64 }) align 8) -> i64, which writes to its copy
    let take_ty = p.m.types.function(I64, &[PTR], false).unwrap();
    let desc =
        FunctionDesc { param_attrs: &[byval], has_body: true, ..FunctionDesc::new(take_ty) };
    let take = p.m.add_function(b"take", &desc).unwrap();
    let mut x = Fx::new(&mut p.m, take);
    let first = x.load(I64, x.arg(0));
    let second_place = x.ptr_add(x.arg(0), 8, &[]);
    let second = x.load(I64, second_place);
    let hundred = x.i64(100);
    x.store(hundred, x.arg(0));
    let sum = x.bin(Opcode::Add, I64, first, second);
    x.ret(sum);
    let body = x.done();
    p.bodies.push(body);

    // fill(ptr sret({ i64, i64 }) align 8, i64)
    let fill_ty = p.m.types.function(TypeId::VOID, &[PTR, I64], false).unwrap();
    let desc = FunctionDesc {
        param_attrs: &[sret, ParamAttrs::NONE],
        has_body: true,
        ..FunctionDesc::new(fill_ty)
    };
    let fill = p.m.add_function(b"fill", &desc).unwrap();
    let mut x = Fx::new(&mut p.m, fill);
    x.store(x.arg(1), x.arg(0));
    let one = x.i64(1);
    let next = x.bin(Opcode::Add, I64, x.arg(1), one);
    let second_place = x.ptr_add(x.arg(0), 8, &[]);
    x.store(next, second_place);
    x.ret_void();
    let body = x.done();
    p.bodies.push(body);

    // fast(i64, i64) -> i64, fastcc and internal
    let fast_ty = p.m.types.function(I64, &[I64, I64], false).unwrap();
    let desc = FunctionDesc {
        linkage: Linkage::Internal,
        cc: CallConv::FAST,
        has_body: true,
        ..FunctionDesc::new(fast_ty)
    };
    let fast = p.m.add_function(b"fast", &desc).unwrap();
    let mut x = Fx::new(&mut p.m, fast);
    let difference = x.bin(Opcode::Sub, I64, x.arg(0), x.arg(1));
    x.ret(difference);
    let body = x.done();
    p.bodies.push(body);

    let mut x = p.body("conventions", I64, &[I64]);
    let given = x.b.add_slot(16, 8).unwrap();
    let filled = x.b.add_slot(16, 8).unwrap();
    let (given, filled) = (x.b.slot(given).unwrap(), x.b.slot(filled).unwrap());
    let (minus_five, big) = (x.int(I8, -5), x.int(i16_ty, -1));
    let narrowed = x
        .b
        .call(
            I32,
            &CallDesc {
                ret_attrs: signext,
                arg_attrs: &[signext, zeroext],
                ..CallDesc::new(Value::sym(narrow).unwrap(), narrow_ty, &[minus_five, big])
            },
        )
        .unwrap();
    let narrowed = x.cast(Opcode::SExt, I64, narrowed);
    let (three, four) = (x.i64(3), x.i64(4));
    x.store(three, given);
    let given_second = x.ptr_add(given, 8, &[]);
    x.store(four, given_second);
    let taken = x
        .b
        .call(
            I64,
            &CallDesc {
                arg_attrs: &[byval],
                ..CallDesc::new(Value::sym(take).unwrap(), take_ty, &[given])
            },
        )
        .unwrap();
    let kept = x.load(I64, given);
    x.b.call(
        TypeId::VOID,
        &CallDesc {
            arg_attrs: &[sret, ParamAttrs::NONE],
            ..CallDesc::new(Value::sym(fill).unwrap(), fill_ty, &[filled, x.arg(0)])
        },
    )
    .unwrap();
    let filled_first = x.load(I64, filled);
    let filled_second_place = x.ptr_add(filled, 8, &[]);
    let filled_second = x.load(I64, filled_second_place);
    let mut sum = narrowed;
    for value in [taken, kept, filled_first, filled_second] {
        let ten = x.i64(10);
        sum = x.bin(Opcode::Mul, I64, sum, ten);
        sum = x.bin(Opcode::Add, I64, sum, value);
    }
    let one = x.i64(1);
    let result = x
        .b
        .call(
            I64,
            &CallDesc {
                cc: CallConv::FAST,
                tail: TailKind::Tail,
                ..CallDesc::new(Value::sym(fast).unwrap(), fast_ty, &[sum, one])
            },
        )
        .unwrap();
    x.ret(result);
    let body = x.done();
    p.bodies.push(body);
}

fn conventions_model(x: i64) -> i64 {
    let narrowed = -5 + 65535;
    let mut sum: i64 = narrowed;
    for value in [3 + 4, 3, x, x + 1] {
        sum = sum * 10 + value;
    }
    sum - 1
}

// ---- main ----

/// The body of `main`: every function called, each result printed with
/// `printf`, and the exclusive or of them all, reduced to seven bits,
/// returned.
fn main_body(p: &mut Program) {
    let printf = declare(&mut p.m, "printf", I32, &[PTR], true);
    let printf_ty = p.m.symbol(printf).ty();
    let format = p.m.consts.bytes(&mut p.m.types, b"%ld\n\0").unwrap();
    let format_ty = p.m.consts.ty(format);
    let desc = VariableDesc {
        linkage: Linkage::Private,
        unnamed_addr: hod_ir::UnnamedAddr::Global,
        constant: true,
        init: Some(format),
        ..VariableDesc::new(format_ty)
    };
    let format = p.m.add_variable(b".str", &desc).unwrap();
    let i16_ty = p.m.types.int(16).unwrap();

    /// One call: the function, how to make its arguments, and its result.
    type Args = fn(&mut Fx<'_>, TypeId) -> Vec<Value>;
    let a = 0xfedc_ba98_7654_3210_u64;
    let b = 0x0123_4567_89ab_cdef_u64;
    let calls: Vec<(&str, Args, i64)> = vec![
        ("sum_squares", |x, _| vec![x.i64(10)], (0..10).map(|i| i * i).sum()),
        ("factorial", |x, _| vec![x.i64(10)], (1..=10).product()),
        ("classify", |x, _| vec![x.i32(0)], classify_model(0)),
        ("classify", |x, _| vec![x.i32(1)], classify_model(1)),
        ("classify", |x, _| vec![x.i32(7)], classify_model(7)),
        ("classify", |x, _| vec![x.i32(-3)], classify_model(-3)),
        ("classify", |x, _| vec![x.i32(5)], classify_model(5)),
        ("apply", |x, _| vec![x.i64(0), x.i64(5)], 6),
        ("apply", |x, _| vec![x.i64(1), x.i64(5)], 10),
        ("apply", |x, _| vec![x.i64(2), x.i64(5)], -5),
        ("middle", |_, _| vec![], 33),
        ("slots", |x, _| vec![x.i64(9)], slots_model(9)),
        ("slots", |x, _| vec![x.i64(-70000)], slots_model(-70000)),
        (
            "ptradd",
            |x, i16_ty| vec![x.int(TypeId::I8, -2), x.int(i16_ty, 3), x.i32(-1), x.i64(1)],
            ptradd_model(-2, 3, -1, 1),
        ),
        ("aggregate", |x, _| vec![x.i64(300)], aggregate_model(300)),
        ("aggregate", |x, _| vec![x.i64(-2)], aggregate_model(-2)),
        (
            "wide",
            |x, _| vec![x.i64(0xfedc_ba98_7654_3210_u64 as i64), x.i64(0x0123_4567_89ab_cdef)],
            wide_model(a, b),
        ),
        ("floats", |x, _| vec![x.f64(2.25)], floats_model(2.25)),
        ("floats", |x, _| vec![x.f64(-12345.678)], floats_model(-12345.678)),
        ("dynamic", |x, _| vec![x.i64(6)], 6 + 12 + 6),
        ("atomics", |_, _| vec![], 5 + 12 * 10 + 1000 + 99 * 100_000),
        ("vectors", |x, _| vec![x.i32(3)], 2 * (3 + 1 + 6 + 4)),
        ("dispatch", |x, _| vec![x.i64(0)], 111),
        ("dispatch", |x, _| vec![x.i64(1)], 222),
        (
            "intrinsics",
            |x, _| vec![x.i32(0x00f0_f00f), x.i64(13)],
            intrinsics_model(0x00f0_f00f, 13),
        ),
        ("counter", |_, _| vec![], 18),
        ("counter", |_, _| vec![], 19),
        ("misc", |x, _| vec![x.i32(-1000)], misc_model(-1000)),
        ("misc", |x, _| vec![x.i32(123_456)], misc_model(123_456)),
        ("anonymous", |x, _| vec![x.i64(4)], 17),
        ("conventions", |x, _| vec![x.i64(6)], conventions_model(6)),
    ];

    let functions: Vec<SymId> = calls.iter().map(|(name, ..)| p.function(name)).collect();
    let mut expected = String::new();
    let mut check = 0;
    let mut x = p.body("main", I32, &[]);
    let mut all = x.i64(0);
    for ((_, args, result), function) in calls.iter().zip(functions) {
        let args = args(&mut x, i16_ty);
        let value = x.call(function, &args);
        let text = x.sym(format);
        x.b.call(I32, &CallDesc::new(Value::sym(printf).unwrap(), printf_ty, &[text, value]))
            .unwrap();
        all = x.bin(Opcode::Xor, I64, all, value);
        writeln!(expected, "{result}").unwrap();
        check ^= result;
    }
    let mask = x.i64(0x7f);
    let low = x.bin(Opcode::And, I64, all, mask);
    let code = x.cast(Opcode::Trunc, I32, low);
    x.ret(code);
    let body = x.done();
    p.bodies.push(body);
    p.expected = expected;
    p.check = check;
}

fn program() -> Program {
    let mut p = Program { m: new_module(), bodies: Vec::new(), expected: String::new(), check: 0 };
    sum_squares(&mut p);
    factorial(&mut p);
    classify(&mut p);
    apply(&mut p);
    slots(&mut p);
    ptradd(&mut p);
    aggregate(&mut p);
    wide(&mut p);
    floats(&mut p);
    dynamic(&mut p);
    atomics(&mut p);
    vectors(&mut p);
    dispatch(&mut p);
    intrinsics(&mut p);
    counter(&mut p);
    misc(&mut p);
    anonymous(&mut p);
    conventions(&mut p);
    main_body(&mut p);
    p.m.verify_symbols().unwrap();
    p
}

/// Requires that the program made of `objects` prints and returns what
/// `p` says.
fn expect_run(scratch: &Scratch, name: &str, objects: &[std::path::PathBuf], p: &Program) -> bool {
    let Some(outcome) = link_and_run(scratch, name, objects, &[]) else { return false };
    assert_eq!(outcome.stdout_text(), p.expected, "{name}: what the program printed");
    let code = outcome.status.and_then(|status| status.code());
    assert_eq!(code, Some((p.check & 0x7f) as i32), "{name}: what the program returned");
    true
}

/// §9.4: the program compiled by `llc -O0` and `llc -O2` of LLVM 22 and
/// LLVM 19 prints and returns what the IR says.
#[test]
fn printed_programs_run_as_the_ir_says() {
    if !host_is_aarch64_linux() {
        return;
    }
    let p = program();
    assert!(p.expected.lines().count() > 30);
    let scratch = Scratch::new("print-run");
    for major in hod_testutil::LLVM_MAJORS {
        let Some(llvm) = Llvm::new(major) else { continue };
        let text = printed(&p.m, llvm.dialect(), &p.bodies);
        if let Ok(dir) = std::env::var("HOD_WP006_DUMP") { std::fs::write(format!("{dir}/run{major}.ll"), &text).unwrap(); }
        llvm.accept(&text, "the program");
        for level in ["-O0", "-O2"] {
            let name = format!("run-{major}{level}");
            let object = llvm.object(&scratch, &name, &text, &[level, "-relocation-model=pic"]);
            if expect_run(&scratch, &name, &[object], &p) {
                eprintln!(
                    "print_run: LLVM {major} {level}: {} functions, {} lines printed, exit {}",
                    p.bodies.len(),
                    p.expected.lines().count(),
                    p.check & 0x7f
                );
            }
        }
    }
}

/// §9.6: the program split into two texts, the bodies dealt out between
/// them, links to a program that prints and returns the same. The local
/// symbols that cross the split (internal functions and variables, the
/// anonymous ones, the private format string) are found by both objects.
#[test]
fn a_split_module_links_to_the_same_program() {
    if !host_is_aarch64_linux() {
        return;
    }
    let p = program();
    // `dispatch` stays with the table of its block addresses, which a
    // `Part::Main` text defines (§9.6).
    let dispatch = p.function("dispatch");
    let in_rest = |at: usize, body: &Body| at % 2 == 1 && body.symbol() != dispatch;
    let rest: Vec<&Body> =
        p.bodies.iter().enumerate().filter(|(at, body)| in_rest(*at, body)).map(|(_, b)| b).collect();
    let main: Vec<&Body> =
        p.bodies.iter().enumerate().filter(|(at, body)| !in_rest(*at, body)).map(|(_, b)| b).collect();
    assert!(rest.len() >= 8 && main.len() >= 8);
    let scratch = Scratch::new("print-split");
    for major in hod_testutil::LLVM_MAJORS {
        let Some(llvm) = Llvm::new(major) else { continue };
        let mut objects = Vec::new();
        for (part, bodies, name) in [(Part::Main, &main, "main"), (Part::Rest, &rest, "rest")] {
            let mut text = String::new();
            p.m.print_part(&mut text, llvm.dialect(), part, bodies.iter().copied()).unwrap();
            llvm.accept(&text, name);
            let name = format!("split-{major}-{name}");
            objects.push(llvm.object(&scratch, &name, &text, &["-O0", "-relocation-model=pic"]));
        }
        if expect_run(&scratch, &format!("split-{major}"), &objects, &p) {
            eprintln!(
                "print_run: LLVM {major}: split into {} and {} functions, same output",
                main.len(),
                rest.len()
            );
        }
    }
}

// ---- exceptions ----

/// A program that throws an `int` through a frame with a cleanup and
/// catches it: `thrower(v)` calls `__cxa_throw`; `middle(v)` invokes it
/// with a cleanup landing pad that counts and resumes; `catcher(v)`
/// invokes that with a `catch` clause for `int`, tests the selector with
/// `llvm.eh.typeid.for`, and reads the exception.
fn exception_program() -> (Module, Vec<Body>) {
    let mut m = new_module();
    let mut bodies = Vec::new();
    let allocate = declare(&mut m, "__cxa_allocate_exception", PTR, &[I64], false);
    let throw = declare(&mut m, "__cxa_throw", TypeId::VOID, &[PTR, PTR, PTR], false);
    let begin_catch = declare(&mut m, "__cxa_begin_catch", PTR, &[PTR], false);
    let end_catch = declare(&mut m, "__cxa_end_catch", TypeId::VOID, &[], false);
    let personality = declare(&mut m, "__gxx_personality_v0", I32, &[], true);
    let type_info =
        m.add_variable(b"_ZTIi", &VariableDesc { constant: true, ..VariableDesc::new(PTR) }).unwrap();
    let zero = int(&mut m, I64, 0);
    let cleanups = m
        .add_variable(
            b"cleanups",
            &VariableDesc { linkage: Linkage::Internal, init: Some(zero), ..VariableDesc::new(I64) },
        )
        .unwrap();
    let pad_ty = m.types.struct_type(&[PTR, I32], false).unwrap();
    let with_personality = |m: &mut Module, name: &str, ret: TypeId| {
        let ty = m.types.function(ret, &[I32], false).unwrap();
        let desc = FunctionDesc {
            personality: Some(personality),
            has_body: true,
            ..FunctionDesc::new(ty)
        };
        m.add_function(name.as_bytes(), &desc).unwrap()
    };

    let thrower = define(&mut m, "thrower", TypeId::VOID, &[I32]);
    let mut x = Fx::new(&mut m, thrower);
    let four = x.i64(4);
    let exception = x.call(allocate, &[four]);
    x.store(x.arg(0), exception);
    let (info, null) = (x.sym(type_info), x.null());
    let throw_ty = x.m.symbol(throw).ty();
    x.b.call(
        TypeId::VOID,
        &CallDesc {
            noreturn: true,
            ..CallDesc::new(Value::sym(throw).unwrap(), throw_ty, &[exception, info, null])
        },
    )
    .unwrap();
    x.b.unreachable().unwrap();
    bodies.push(x.done());

    let middle = with_personality(&mut m, "middle", TypeId::VOID);
    let thrower_ty = m.symbol(thrower).ty();
    let mut x = Fx::new(&mut m, middle);
    let (normal, pad) = (x.block(), x.block());
    let callee = Value::sym(thrower).unwrap();
    x.b.invoke(TypeId::VOID, &CallDesc::new(callee, thrower_ty, &[x.arg(0)]), normal, pad).unwrap();
    x.at(normal);
    x.ret_void();
    x.at(pad);
    let caught = x.b.landing_pad(pad_ty, true, &[]).unwrap();
    let count = x.load(I64, x.sym(cleanups));
    let one = x.i64(1);
    let count = x.bin(Opcode::Add, I64, count, one);
    x.store(count, x.sym(cleanups));
    x.b.resume(caught).unwrap();
    bodies.push(x.done());

    let catcher = with_personality(&mut m, "catcher", I64);
    let middle_ty = m.symbol(middle).ty();
    let info = m.consts.addr(type_info, 0).unwrap();
    let mut x = Fx::new(&mut m, catcher);
    let (normal, pad, matched, other) = (x.block(), x.block(), x.block(), x.block());
    let callee = Value::sym(middle).unwrap();
    x.b.invoke(TypeId::VOID, &CallDesc::new(callee, middle_ty, &[x.arg(0)]), normal, pad).unwrap();
    x.at(normal);
    let nothing = x.i64(-1);
    x.ret(nothing);
    x.at(pad);
    let caught = x.b.landing_pad(pad_ty, false, &[Clause::Catch(info)]).unwrap();
    let exception = x.b.extract_value(PTR, caught, 0).unwrap();
    let selector = x.b.extract_value(I32, caught, 8).unwrap();
    let wanted = x.intrinsic(Intrinsic::EhTypeidFor, &[PTR], I32, &[PTR], &[x.sym(type_info)]);
    let is_int = x.icmp(IntPredicate::Eq, selector, wanted);
    x.cond_br(is_int, matched, other);
    x.at(matched);
    let object = x.call(begin_catch, &[exception]);
    let value = x.load(I32, object);
    x.call(end_catch, &[]);
    let value = x.cast(Opcode::SExt, I64, value);
    let thousand = x.i64(1000);
    let cleanups_run = x.load(I64, x.sym(cleanups));
    let scaled = x.bin(Opcode::Mul, I64, cleanups_run, thousand);
    let result = x.bin(Opcode::Add, I64, value, scaled);
    x.ret(result);
    x.at(other);
    x.b.resume(caught).unwrap();
    bodies.push(x.done());

    let printf = declare(&mut m, "printf", I32, &[PTR], true);
    let printf_ty = m.symbol(printf).ty();
    let format = m.consts.bytes(&mut m.types, b"%ld\n\0").unwrap();
    let format_ty = m.consts.ty(format);
    let desc = VariableDesc {
        linkage: Linkage::Private,
        constant: true,
        init: Some(format),
        ..VariableDesc::new(format_ty)
    };
    let format = m.add_variable(b".str", &desc).unwrap();
    let main = define(&mut m, "main", I32, &[]);
    let mut x = Fx::new(&mut m, main);
    let seventy_seven = x.i32(77);
    let result = x.call(catcher, &[seventy_seven]);
    let text = x.sym(format);
    x.b.call(I32, &CallDesc::new(Value::sym(printf).unwrap(), printf_ty, &[text, result])).unwrap();
    let code = x.i32(7);
    x.ret(code);
    bodies.push(x.done());
    m.verify_symbols().unwrap();
    (m, bodies)
}

/// §9.4: `invoke`, `landingpad` (a cleanup and a `catch` clause) and
/// `resume` as printed unwind and catch a C++ exception: `llc -O0` of
/// LLVM 22 and `llc -O2` of LLVM 19, linked with the C++ runtime.
#[test]
fn printed_landing_pads_catch_a_thrown_exception() {
    if !host_is_aarch64_linux() {
        return;
    }
    let (m, bodies) = exception_program();
    let scratch = Scratch::new("print-eh");
    for (major, level) in [(22, "-O0"), (19, "-O2")] {
        let Some(llvm) = Llvm::new(major) else { continue };
        let text = printed(&m, Dialect::new(major).unwrap(), &bodies);
        llvm.accept(&text, "the exception program");
        let name = format!("eh-{major}");
        let object = llvm.object(&scratch, &name, &text, &[level, "-relocation-model=pic"]);
        let Some(outcome) = link_and_run(&scratch, &name, &[object], &["-lstdc++"]) else {
            continue;
        };
        // One cleanup ran, then the catch read the 77.
        assert_eq!(outcome.stdout_text(), "1077\n", "LLVM {major} {level}");
        assert_eq!(outcome.status.and_then(|status| status.code()), Some(7));
        eprintln!("print_run: LLVM {major} {level}: an exception thrown, cleaned up after, caught");
    }
}
