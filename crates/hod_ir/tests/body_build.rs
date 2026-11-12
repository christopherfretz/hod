//! L0 tests of function bodies (`spec/ir.md` §5): every instruction group
//! is built with `FunctionBuilder` and read back through the API code
//! generation uses, every builder error is provoked, and `Body::verify`
//! is run on every body, accepting the well-formed ones and naming the
//! rule each ill-formed one breaks.

// The fixture's values are named for their types (`a` an `i32`, `l` an
// `i64`, `p` a pointer, ...), which keeps the instruction lists readable.
#![allow(clippy::many_single_char_names, clippy::similar_names)]

mod body_support;

use body_support::dump;
use hod_ir::{
    AsmId, AtomicOrdering, Block, BlockId, Body, Bundle, CallConv, CallDesc, CallSite, Callee,
    Clause, DiId, FloatPredicate, FunctionBuilder, InlineAsm, Inst, InstId, InstKind, IntPredicate,
    Intrinsic, IrErrorKind, IrResult, LocId, MemFlags, Module, Opcode, ParamAttrs, RmwOp,
    SHUFFLE_UNDEF, SlotId, SymId, SyncScope, TailKind, TargetSpec, TypeId, Value, ValueKind,
    aggregate_field,
};

const I1: TypeId = TypeId::I1;
const I8: TypeId = TypeId::I8;
const I32: TypeId = TypeId::I32;
const I64: TypeId = TypeId::I64;
const FLOAT: TypeId = TypeId::FLOAT;
const DOUBLE: TypeId = TypeId::DOUBLE;
const PTR: TypeId = TypeId::PTR;
const VOID: TypeId = TypeId::VOID;

fn kind<T: std::fmt::Debug>(result: IrResult<T>) -> IrErrorKind {
    result.unwrap_err().kind
}

fn int(m: &mut Module, ty: TypeId, value: u64) -> Value {
    Value::constant(m.consts.int(&m.types, ty, value).unwrap()).unwrap()
}

fn align(bytes: u64) -> MemFlags {
    MemFlags::aligned(bytes).unwrap()
}

fn id(value: Value) -> InstId {
    value.as_inst().unwrap()
}

/// A module and a builder for
/// `void f(i32, i64, float, ptr, <4 x i32>, { i32, i64 }, double, i1)`
/// with its entry block open.
struct Fx {
    m: Module,
    b: FunctionBuilder,
    entry: BlockId,
    a: Value,
    l: Value,
    x: Value,
    p: Value,
    v: Value,
    s: Value,
    d: Value,
    c: Value,
    v4: TypeId,
    st: TypeId,
}

fn fx() -> Fx {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let v4 = m.types.vector(I32, 4).unwrap();
    let st = m.types.struct_type(&[I32, I64], false).unwrap();
    let params = [I32, I64, FLOAT, PTR, v4, st, DOUBLE, I1];
    let fn_ty = m.types.function(VOID, &params, false).unwrap();
    let mut b = FunctionBuilder::new(&m, SymId::new(1), fn_ty).unwrap();
    let entry = b.create_block().unwrap();
    b.begin_block(entry).unwrap();
    let arg = |n| b.arg(n).unwrap();
    let (a, l, x, p, v, s, d, c) = (arg(0), arg(1), arg(2), arg(3), arg(4), arg(5), arg(6), arg(7));
    Fx { m, b, entry, a, l, x, p, v, s, d, c, v4, st }
}

impl Fx {
    /// Ends the open block with `ret void`, finishes and verifies. The
    /// fixture keeps its module and argument values; its builder is
    /// replaced by an empty one.
    fn done(&mut self) -> Body {
        if self.b.current_block().is_some() {
            self.b.ret(None).unwrap();
        }
        let body = self.take().finish().unwrap();
        if let Err(error) = body.verify(&self.m) {
            panic!("{error}\n{}", dump(&body, &self.m));
        }
        body
    }

    /// The builder, leaving an empty one behind.
    fn take(&mut self) -> FunctionBuilder {
        let fn_ty = self.m.types.function(VOID, &[], false).unwrap();
        let empty = FunctionBuilder::new(&self.m, SymId::new(0), fn_ty).unwrap();
        std::mem::replace(&mut self.b, empty)
    }
}

/// A phi's list, given the entry block and the diamond's left block.
type Incoming = dyn Fn(&Fx, BlockId, BlockId) -> Vec<(Value, BlockId)>;

/// What `verify` says about the body `build` makes in the fixture.
fn verdict(build: impl FnOnce(&mut Fx)) -> Option<IrErrorKind> {
    let mut f = fx();
    build(&mut f);
    if f.b.current_block().is_some() {
        f.b.ret(None).unwrap();
    }
    let body = f.b.finish().unwrap();
    body.verify(&f.m).err().map(|error| error.kind)
}

/// §5.4 "integer", "float", "compare", "cast": each opcode and predicate
/// is built and reads back with its operands and type.
#[test]
fn arithmetic_compares_and_casts_read_back() {
    let mut f = fx();
    let b = &mut f.b;
    let mut expected: Vec<(Value, Opcode, TypeId, Vec<Value>)> = Vec::new();
    for &op in Opcode::ALL.iter().filter(|op| op.is_binary()) {
        let (ty, lhs, rhs) = if op.is_int_binary() { (I32, f.a, f.a) } else { (FLOAT, f.x, f.x) };
        expected.push((b.binary(op, ty, lhs, rhs).unwrap(), op, ty, vec![lhs, rhs]));
    }
    expected.push((b.fneg(FLOAT, f.x).unwrap(), Opcode::FNeg, FLOAT, vec![f.x]));
    expected.push((b.freeze(I64, f.l).unwrap(), Opcode::Freeze, I64, vec![f.l]));
    let p1 = f.m.types.ptr(1).unwrap();
    let wide = b.cast(Opcode::FPExt, DOUBLE, f.x).unwrap();
    let casts = [
        (Opcode::Trunc, I8, f.a),
        (Opcode::ZExt, I64, f.a),
        (Opcode::SExt, I64, f.a),
        (Opcode::FPTrunc, FLOAT, wide),
        (Opcode::FPToUI, I32, f.x),
        (Opcode::FPToSI, I32, f.x),
        (Opcode::UIToFP, FLOAT, f.a),
        (Opcode::SIToFP, DOUBLE, f.l),
        (Opcode::PtrToInt, I64, f.p),
        (Opcode::PtrToAddr, I64, f.p),
        (Opcode::IntToPtr, PTR, f.l),
        (Opcode::BitCast, FLOAT, f.a),
        (Opcode::AddrSpaceCast, p1, f.p),
    ];
    expected.push((wide, Opcode::FPExt, DOUBLE, vec![f.x]));
    for (op, ty, arg) in casts {
        expected.push((b.cast(op, ty, arg).unwrap(), op, ty, vec![arg]));
    }
    let mut compares = Vec::new();
    for &pred in IntPredicate::ALL {
        compares.push((b.icmp(pred, I1, f.a, f.a).unwrap(), pred as u8, true));
    }
    for &pred in FloatPredicate::ALL {
        compares.push((b.fcmp(pred, I1, f.x, f.x).unwrap(), pred as u8, false));
    }
    let v4i1 = f.m.types.vector(I1, 4).unwrap();
    let vector_compare = b.icmp(IntPredicate::Slt, v4i1, f.v, f.v).unwrap();

    let body = f.done();
    assert_eq!(expected.len(), 18 + 2 + 14);
    for (value, op, ty, operands) in expected {
        let inst = body.inst(id(value));
        assert_eq!(inst.opcode(), op);
        assert_eq!(inst.ty(), ty);
        assert_eq!(inst.operands(), operands);
        assert_eq!(inst.result(), value);
        assert!(!inst.is_terminator());
        match inst.kind() {
            InstKind::Binary { lhs, rhs } => {
                assert!(op.is_binary());
                assert_eq!(vec![lhs, rhs], operands);
            }
            InstKind::Unary { arg } => {
                assert!(matches!(op, Opcode::FNeg | Opcode::Freeze));
                assert_eq!(vec![arg], operands);
            }
            InstKind::Cast { arg } => {
                assert!(op.is_cast());
                assert_eq!(vec![arg], operands);
            }
            other => panic!("{other:?}"),
        }
    }
    for (value, pred, is_int) in compares {
        match body.inst(id(value)).kind() {
            InstKind::ICmp { pred: found, lhs, rhs } => {
                assert!(is_int);
                assert_eq!((found as u8, lhs, rhs), (pred, f.a, f.a));
            }
            InstKind::FCmp { pred: found, lhs, rhs } => {
                assert!(!is_int);
                assert_eq!((found as u8, lhs, rhs), (pred, f.x, f.x));
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(body.inst(id(vector_compare)).ty(), v4i1);
}

/// §5.4 "memory": alignment, `volatile`, ordering and scope of each
/// access read back, and the dynamic `alloca` (§5.2).
#[test]
fn memory_instructions_read_back() {
    let mut f = fx();
    let b = &mut f.b;
    let plain = align(4);
    let fancy = align(16).with_volatile().with_atomic(AtomicOrdering::Acquire, SyncScope::System);
    let load = b.load(I32, f.p, plain).unwrap();
    let atomic_load = b.load(I64, f.p, fancy).unwrap();
    let store = b.store(f.a, f.p, plain.with_volatile()).unwrap();
    let release = align(8).with_atomic(AtomicOrdering::Release, SyncScope::SingleThread);
    let atomic_store = b.store(f.l, f.p, release).unwrap();
    let dynamic = b.alloca(PTR, f.l, (1 << 40) + 24, 32).unwrap();
    let fence = b.fence(AtomicOrdering::SeqCst, SyncScope::SingleThread).unwrap();
    let pair = f.m.types.struct_type(&[I32, I1], false).unwrap();
    let seq = align(4).with_atomic(AtomicOrdering::SeqCst, SyncScope::System);
    let cas = b.cmpxchg(pair, f.p, f.a, f.a, seq, AtomicOrdering::Monotonic, true).unwrap();
    let mut rmws = Vec::new();
    for &op in RmwOp::ALL {
        let (ty, operand) = match op {
            RmwOp::Xchg => (PTR, f.p),
            op if op.is_float() => (FLOAT, f.x),
            _ => (I32, f.a),
        };
        rmws.push((b.atomic_rmw(op, ty, f.p, operand, seq).unwrap(), op, operand));
    }

    let body = f.done();
    let InstKind::Load { ptr, mem } = body.inst(id(load)).kind() else { panic!() };
    assert_eq!((ptr, mem), (f.p, plain));
    assert_eq!((mem.align(), mem.is_volatile(), mem.ordering()), (4, false, None));
    let InstKind::Load { mem, .. } = body.inst(id(atomic_load)).kind() else { panic!() };
    assert_eq!(mem, fancy);
    assert_eq!((mem.align(), mem.is_volatile()), (16, true));
    assert_eq!(mem.ordering(), Some(AtomicOrdering::Acquire));
    let InstKind::Store { value, ptr, mem } = body.inst(id(store)).kind() else { panic!() };
    assert_eq!((value, ptr, mem.is_volatile(), mem.align()), (f.a, f.p, true, 4));
    assert_eq!(body.inst(id(store)).ty(), VOID);
    assert_eq!(body.inst(id(store)).operands(), [f.a, f.p]);
    let InstKind::Store { mem, .. } = body.inst(id(atomic_store)).kind() else { panic!() };
    assert_eq!(mem.ordering(), Some(AtomicOrdering::Release));
    assert_eq!(mem.scope(), SyncScope::SingleThread);
    let InstKind::Alloca { count, elem_size, align } = body.inst(id(dynamic)).kind() else {
        panic!()
    };
    assert_eq!((count, elem_size, align), (f.l, (1 << 40) + 24, 32));
    assert_eq!(body.inst(id(dynamic)).operands(), [f.l]);
    let InstKind::Fence { ordering, scope } = body.inst(id(fence)).kind() else { panic!() };
    assert_eq!((ordering, scope), (AtomicOrdering::SeqCst, SyncScope::SingleThread));
    assert!(body.inst(id(fence)).operands().is_empty());
    let InstKind::CmpXchg { ptr, expected, replacement, mem, failure, weak } =
        body.inst(id(cas)).kind()
    else {
        panic!()
    };
    assert_eq!((ptr, expected, replacement), (f.p, f.a, f.a));
    assert_eq!(mem, seq);
    assert_eq!((failure, weak), (AtomicOrdering::Monotonic, true));
    for (value, op, operand) in rmws {
        let InstKind::AtomicRmw { op: found, ptr, value: found_operand, mem } =
            body.inst(id(value)).kind()
        else {
            panic!()
        };
        assert_eq!((found, ptr, found_operand, mem), (op, f.p, operand, seq));
    }
}

/// §5.4 "address", "aggregate": `PtrAdd` in both forms, the aggregate
/// instructions with offsets from `aggregate_field`, the vector ones.
#[test]
fn addresses_and_aggregates_read_back() {
    let mut f = fx();
    let b = &mut f.b;
    let near = b.ptr_add(PTR, f.p, -8, &[]).unwrap();
    let far = b.ptr_add(PTR, f.p, i64::MIN, &[]).unwrap();
    let scaled = b.ptr_add(PTR, near, 12, &[(f.a, 4), (f.l, (1 << 40) + 1)]).unwrap();
    let (offset, field) = aggregate_field(&f.m.types, f.st, &[1]).unwrap();
    assert_eq!((offset, field), (8, I64));
    let got = b.extract_value(I64, f.s, offset).unwrap();
    let put = b.insert_value(f.st, f.s, got, offset).unwrap();
    let first = b.extract_value(I32, put, 0).unwrap();
    let element = b.extract_element(I32, f.v, f.l).unwrap();
    let inserted = b.insert_element(f.v4, f.v, element, f.a).unwrap();
    let v2 = f.m.types.vector(I32, 2).unwrap();
    let mask = [7, SHUFFLE_UNDEF];
    let shuffled = b.shuffle_vector(v2, f.v, inserted, &mask).unwrap();
    let pv = f.m.types.vector(PTR, 4).unwrap();
    let spread = b.ptr_add(pv, f.p, 0, &[(f.v, 8)]).unwrap();

    let body = f.done();
    let InstKind::PtrAdd(add) = body.inst(id(near)).kind() else { panic!() };
    assert_eq!((add.base, add.offset, add.index_count()), (f.p, -8, 0));
    assert_eq!(body.inst(id(near)).operands(), [f.p]);
    let InstKind::PtrAdd(add) = body.inst(id(far)).kind() else { panic!() };
    assert_eq!(add.offset, i64::MIN);
    let InstKind::PtrAdd(add) = body.inst(id(scaled)).kind() else { panic!() };
    assert_eq!((add.base, add.offset), (near, 12));
    assert_eq!(add.indices().collect::<Vec<_>>(), [(f.a, 4), (f.l, (1 << 40) + 1)]);
    assert_eq!(body.inst(id(scaled)).operands(), [near, f.a, f.l]);
    let InstKind::PtrAdd(add) = body.inst(id(spread)).kind() else { panic!() };
    assert_eq!(add.indices().collect::<Vec<_>>(), [(f.v, 8)]);

    let InstKind::ExtractValue { agg, offset } = body.inst(id(got)).kind() else { panic!() };
    assert_eq!((agg, offset), (f.s, 8));
    let InstKind::InsertValue { agg, value, offset } = body.inst(id(put)).kind() else { panic!() };
    assert_eq!((agg, value, offset), (f.s, got, 8));
    assert_eq!(body.inst(id(put)).operands(), [f.s, got]);
    assert_eq!(body.inst(id(first)).ty(), I32);
    let InstKind::ExtractElement { vector, index } = body.inst(id(element)).kind() else {
        panic!()
    };
    assert_eq!((vector, index), (f.v, f.l));
    let InstKind::InsertElement { vector, value, index } = body.inst(id(inserted)).kind() else {
        panic!()
    };
    assert_eq!((vector, value, index), (f.v, element, f.a));
    let InstKind::ShuffleVector { first, second, mask: found } = body.inst(id(shuffled)).kind()
    else {
        panic!()
    };
    assert_eq!((first, second, found), (f.v, inserted, &mask[..]));
    assert_eq!(body.inst(id(shuffled)).operands(), [f.v, inserted]);
}

/// §5.4 "call", §5.5, §5.7: the call side table holds the callee, the
/// function type, the convention, the tail kind, the three attributes,
/// the parameter attributes and the bundles, and is iterable on its own.
#[test]
fn calls_read_back_from_the_side_table() {
    let mut f = fx();
    let callee_ty = f.m.types.function(I32, &[I32, PTR], true).unwrap();
    let void_ty = f.m.types.function(VOID, &[], false).unwrap();
    let memcpy_ty = f.m.types.function(VOID, &[PTR, PTR, I64, I1], false).unwrap();
    let no = int(&mut f.m, I1, 0);
    let asm = InlineAsm { template: b"nop", constraints: b"", flags: 0, ty: void_ty };
    let asm_id = f.m.inline_asm(&asm).unwrap();
    let b = &mut f.b;
    let direct = Value::sym(SymId::new(9)).unwrap();

    let plain = b.call(VOID, &CallDesc::new(direct, void_ty, &[])).unwrap();

    let signext = ParamAttrs::NONE.with(ParamAttrs::SIGNEXT).unwrap();
    let zeroext = ParamAttrs::NONE.with(ParamAttrs::ZEROEXT).unwrap();
    let args = [f.a, f.p, f.l];
    let inputs = [f.a, f.l];
    let bundles =
        [Bundle { tag: "deopt", inputs: &inputs }, Bundle { tag: "hod.test", inputs: &[] }];
    let rich = b
        .call(
            I32,
            &CallDesc {
                cc: CallConv::FAST,
                tail: TailKind::Tail,
                noreturn: true,
                returns_twice: true,
                ret_attrs: signext,
                arg_attrs: &[zeroext, ParamAttrs::NONE, ParamAttrs::NONE],
                bundles: &bundles,
                ..CallDesc::new(f.p, callee_ty, &args)
            },
        )
        .unwrap();

    let overloads = [PTR, PTR, I64];
    let copy_args = [f.p, f.p, f.l, no];
    let copy = b
        .call(
            VOID,
            &CallDesc {
                nounwind: true,
                ..CallDesc::intrinsic(Intrinsic::Memcpy, &overloads, memcpy_ty, &copy_args)
            },
        )
        .unwrap();
    let unknown = Intrinsic::Other("llvm.aarch64.hint");
    let other = b.call(VOID, &CallDesc::intrinsic(unknown, &[], void_ty, &[])).unwrap();
    let asm = Value::asm(asm_id).unwrap();
    let inline = b.call(VOID, &CallDesc::new(asm, void_ty, &[])).unwrap();

    let body = f.done();
    let calls: Vec<_> = body.calls().collect();
    assert_eq!(body.call_count(), 5);
    let sites: Vec<_> = calls.iter().map(CallSite::inst).collect();
    assert_eq!(sites, [id(plain), id(rich), id(copy), id(other), id(inline)]);
    for call in &calls {
        // The instruction leads to the same record.
        let via = body.inst(call.inst()).call().unwrap();
        assert_eq!(via.inst(), call.inst());
        assert!(matches!(body.inst(call.inst()).kind(), InstKind::Call(_)));
        assert_eq!(call.opcode(), Opcode::Call);
        assert_eq!((call.normal_dest(), call.unwind_dest()), (None, None));
        assert_eq!(call.indirect_dests().len(), 0);
    }

    let call = calls[0];
    assert_eq!(call.callee(), Callee::Value(direct));
    assert_eq!((call.fn_type(), call.ty(), call.cc()), (void_ty, VOID, CallConv::C));
    assert_eq!(call.tail(), TailKind::None);
    assert!(!call.is_noreturn() && !call.is_nounwind() && !call.returns_twice());
    assert!(call.args().is_empty() && !call.has_param_attrs() && call.bundle_count() == 0);
    assert!(call.ret_attrs().is_none() && call.arg_attrs(0).is_none());
    assert_eq!(body.inst(call.inst()).operands(), [direct]);

    let call = calls[1];
    assert_eq!(call.callee(), Callee::Value(f.p));
    assert_eq!(call.callee_value(), f.p);
    assert_eq!((call.fn_type(), call.ty(), call.cc()), (callee_ty, I32, CallConv::FAST));
    assert_eq!(call.tail(), TailKind::Tail);
    assert!(call.is_noreturn() && !call.is_nounwind() && call.returns_twice());
    assert_eq!(call.args(), args);
    assert!(call.has_param_attrs());
    assert_eq!(call.ret_attrs(), signext);
    assert_eq!(call.arg_attrs(0), zeroext);
    assert!(call.arg_attrs(1).is_none() && call.arg_attrs(2).is_none());
    assert!(call.arg_attrs(3).is_none(), "out of range is no attributes");
    let found: Vec<_> = call.bundles().collect();
    assert_eq!(found, bundles);
    assert_eq!(body.inst(call.inst()).operands(), [f.p, f.a, f.p, f.l, f.a, f.l]);

    let call = calls[2];
    assert_eq!(call.callee(), Callee::Intrinsic(Intrinsic::Memcpy));
    assert_eq!(call.intrinsic(), Some(Intrinsic::Memcpy));
    assert!(call.callee_value().is_none());
    assert_eq!(call.overloads(), overloads);
    assert_eq!(call.args(), copy_args);
    assert!(call.is_nounwind());

    let call = calls[3];
    assert_eq!(call.callee(), Callee::Intrinsic(unknown));
    assert!(call.overloads().is_empty());

    let call = calls[4];
    assert_eq!(call.callee(), Callee::Value(asm));
    assert_eq!(asm.kind(), ValueKind::Asm(asm_id));
}

/// §5.4 "exception", "terminator": `invoke`, `landingpad` with its
/// clauses, `resume`, `callbr`, and the landing-pad side table.
#[test]
fn invoke_landing_pad_and_callbr_read_back() {
    let mut f = fx();
    let void_ty = f.m.types.function(VOID, &[], false).unwrap();
    let exn = f.m.types.struct_type(&[PTR, I32], false).unwrap();
    let null = f.m.consts.null(&f.m.types, PTR).unwrap();
    let filter_ty = f.m.types.array(PTR, 1).unwrap();
    let filter = f.m.consts.zero(&f.m.types, filter_ty).unwrap();
    // `callbr` calls inline assembly with a label for each indirect
    // destination (§5.8).
    let goto = InlineAsm { template: b"", constraints: b"!i,!i", flags: 0, ty: void_ty };
    let goto = f.m.inline_asm(&goto).unwrap();
    let b = &mut f.b;
    let [normal, pad_block, after, side] = [(); 4].map(|()| b.create_block().unwrap());
    let callee = Value::sym(SymId::new(2)).unwrap();
    let invoke = b.invoke(VOID, &CallDesc::new(callee, void_ty, &[]), normal, pad_block).unwrap();
    assert_eq!(b.current_block(), None, "an invoke ends its block");
    b.begin_block(normal).unwrap();
    let asm = Value::asm(goto).unwrap();
    let callbr = b.callbr(VOID, &CallDesc::new(asm, void_ty, &[]), after, &[side, after]).unwrap();
    b.begin_block(pad_block).unwrap();
    let clauses = [Clause::Catch(null), Clause::Filter(filter)];
    let pad = b.landing_pad(exn, true, &clauses).unwrap();
    let resume = b.resume(pad).unwrap();
    b.begin_block(after).unwrap();
    b.ret(None).unwrap();
    b.begin_block(side).unwrap();
    b.unreachable().unwrap();

    let body = f.done();
    let call = body.inst(id(invoke)).call().unwrap();
    assert_eq!(call.opcode(), Opcode::Invoke);
    assert_eq!((call.normal_dest(), call.unwind_dest()), (Some(normal), Some(pad_block)));
    assert!(body.inst(id(invoke)).is_terminator());
    assert_eq!(body.inst(id(invoke)).successors().collect::<Vec<_>>(), [normal, pad_block]);
    assert_eq!(body.block(f.entry).successors().collect::<Vec<_>>(), [normal, pad_block]);

    let call = body.inst(id(callbr)).call().unwrap();
    assert_eq!(call.opcode(), Opcode::CallBr);
    assert_eq!((call.normal_dest(), call.unwind_dest()), (Some(after), None));
    assert_eq!(call.indirect_dests().collect::<Vec<_>>(), [side, after]);
    let successors = body.inst(id(callbr)).successors();
    assert_eq!(successors.len(), 3);
    assert_eq!(successors.collect::<Vec<_>>(), [after, side, after]);

    assert!(body.has_landing_pads());
    let pads: Vec<_> = body.landing_pads().collect();
    assert_eq!(pads.len(), 1);
    assert_eq!(pads[0].inst(), id(pad));
    assert!(pads[0].is_cleanup());
    assert_eq!(pads[0].clauses(), clauses);
    let InstKind::LandingPad(found) = body.inst(id(pad)).kind() else { panic!() };
    assert_eq!((found.inst(), found.clauses()), (id(pad), &clauses[..]));
    assert!(body.inst(id(pad)).operands().is_empty());
    let InstKind::Resume { value } = body.inst(id(resume)).kind() else { panic!() };
    assert_eq!(value, pad);
    assert_eq!(body.inst(id(resume)).successors().len(), 0);
    assert_eq!(body.calls().map(|call| call.inst()).collect::<Vec<_>>(), [id(invoke), id(callbr)]);
}

/// §5.1, §5.4 "terminator": blocks are read in layout order, which need
/// not be creation order; each terminator lists its successors in order,
/// repeats included.
#[test]
fn terminators_layout_and_successors() {
    let mut f = fx();
    let seven = f.m.consts.int(&f.m.types, I32, 7).unwrap();
    let nine = f.m.consts.int(&f.m.types, I32, 9).unwrap();
    let b = &mut f.b;
    let [b1, b2, b3, b4, b5] = [(); 5].map(|()| b.create_block().unwrap());
    assert_eq!(b.block_count(), 6);
    let cond = b.cond_br(f.c, b3, b1).unwrap();
    // Laid out b3, b1, b5, b2, b4: not the order of creation.
    b.begin_block(b3).unwrap();
    let switch = b.switch(f.a, b5, &[(seven, b1), (nine, b5), (seven, b1)]).unwrap();
    b.begin_block(b1).unwrap();
    let br = b.br(b5).unwrap();
    b.begin_block(b5).unwrap();
    let indirect = b.indirect_br(f.p, &[b2, b4, b2]).unwrap();
    b.begin_block(b2).unwrap();
    let ret = b.ret(None).unwrap();
    b.begin_block(b4).unwrap();
    let unreachable = b.unreachable().unwrap();
    let body = f.b.finish().unwrap();

    assert_eq!(body.layout(), [f.entry, b3, b1, b5, b2, b4]);
    assert_eq!(body.entry(), f.entry);
    assert_eq!(body.block_count(), 6);
    let order: Vec<_> = body.blocks().map(Block::id).collect();
    assert_eq!(order, body.layout());
    // Instruction ids are layout positions.
    let mut next = 0;
    for block in body.blocks() {
        assert_eq!(block.inst_range().start, next);
        assert_eq!(block.insts().len(), block.inst_range().len());
        assert!(block.terminator().is_terminator());
        assert_eq!(block.terminator().id().raw() + 1, block.inst_range().end);
        assert!(!block.has_phis() && block.phis().len() == 0);
        assert_eq!(block.non_phis().len(), block.insts().len());
        next = block.inst_range().end;
    }
    assert_eq!(next as usize, body.inst_count());
    assert_eq!(body.insts().len(), body.inst_count());

    let successors = |value: Value| body.inst(id(value)).successors().collect::<Vec<_>>();
    assert_eq!(successors(cond), [b3, b1]);
    assert_eq!(successors(switch), [b5, b1, b5, b1]);
    assert_eq!(successors(br), [b5]);
    assert_eq!(successors(indirect), [b2, b4, b2]);
    assert!(successors(ret).is_empty() && successors(unreachable).is_empty());

    let InstKind::CondBr { cond: found, if_true, if_false } = body.inst(id(cond)).kind() else {
        panic!()
    };
    assert_eq!((found, if_true, if_false), (f.c, b3, b1));
    assert_eq!(body.inst(id(cond)).operands(), [f.c]);
    let InstKind::Switch(found) = body.inst(id(switch)).kind() else { panic!() };
    assert_eq!((found.cond, found.default, found.case_count()), (f.a, b5, 3));
    assert_eq!(found.cases().collect::<Vec<_>>(), [(seven, b1), (nine, b5), (seven, b1)]);
    assert_eq!(body.inst(id(switch)).operands(), [f.a]);
    let InstKind::Br { target } = body.inst(id(br)).kind() else { panic!() };
    assert_eq!(target, b5);
    let InstKind::IndirectBr { addr, targets } = body.inst(id(indirect)).kind() else { panic!() };
    assert_eq!((addr, targets.collect::<Vec<_>>()), (f.p, vec![b2, b4, b2]));
    assert!(matches!(body.inst(id(ret)).kind(), InstKind::Ret { value: None }));
    assert!(matches!(body.inst(id(unreachable)).kind(), InstKind::Unreachable));
    // Two cases with one value: the builder takes it, verify does not.
    assert_eq!(kind(body.verify(&f.m)), IrErrorKind::InvalidOperand);
}

/// §5.4: a returned value, `select`, `va_arg`, and the funclet opcodes,
/// which carry nothing.
#[test]
fn remaining_instructions_read_back() {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let fn_ty = m.types.function(I32, &[I1, I32, PTR], false).unwrap();
    let mut b = FunctionBuilder::new(&m, SymId::new(0), fn_ty).unwrap();
    assert_eq!(b.arg_count(), 3);
    let (c, a, p) = (b.arg(0).unwrap(), b.arg(1).unwrap(), b.arg(2).unwrap());
    let entry = b.create_block().unwrap();
    let handler = b.create_block().unwrap();
    b.begin_block(entry).unwrap();
    let next = b.va_arg(I32, p).unwrap();
    let chosen = b.select(I32, c, a, next).unwrap();
    let pad = b.funclet(Opcode::CleanupPad, TypeId::TOKEN).unwrap();
    let ret = b.ret(Some(chosen)).unwrap();
    b.begin_block(handler).unwrap();
    let switch = b.funclet(Opcode::CatchSwitch, TypeId::TOKEN).unwrap();
    assert_eq!(b.current_block(), None, "catchswitch is a terminator");
    let body = b.finish().unwrap();
    body.verify(&m).unwrap();

    assert_eq!((body.symbol(), body.fn_type()), (SymId::new(0), fn_ty));
    assert_eq!(body.arg_types(&m), [I1, I32, PTR]);
    assert_eq!(body.ret_type(&m), I32);
    let InstKind::VaArg { list } = body.inst(id(next)).kind() else { panic!() };
    assert_eq!(list, p);
    let InstKind::Select { cond, if_true, if_false } = body.inst(id(chosen)).kind() else {
        panic!()
    };
    assert_eq!((cond, if_true, if_false), (c, a, next));
    assert_eq!(body.inst(id(chosen)).operands(), [c, a, next]);
    let InstKind::Ret { value } = body.inst(id(ret)).kind() else { panic!() };
    assert_eq!(value, Some(chosen));
    assert_eq!(body.inst(id(ret)).operands(), [chosen]);
    for (value, op) in [(pad, Opcode::CleanupPad), (switch, Opcode::CatchSwitch)] {
        let inst = body.inst(id(value));
        assert_eq!(inst.opcode(), op);
        assert!(matches!(inst.kind(), InstKind::Funclet));
        assert!(inst.operands().is_empty() && inst.successors().len() == 0);
    }
    assert_eq!(body.value_type(&m, chosen), I32);
    assert_eq!(body.value_type(&m, c), I1);
    assert_eq!(body.value_type(&m, Value::sym(SymId::new(5)).unwrap()), PTR);
    assert_eq!(body.value_type(&m, Value::NONE), VOID);
    let one = int(&mut m, I64, 1);
    assert_eq!(body.value_type(&m, one), I64);
}

/// §5.3: a value used before it is defined is reserved, and every use of
/// it names the definition in the finished body. §5.4: `phi` lists can
/// be given at once or grown later, in any order.
#[test]
fn phis_and_forward_references() {
    // i32 f(i32 n): a counting loop whose header is laid out first.
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let fn_ty = m.types.function(I32, &[I32], false).unwrap();
    let zero = int(&mut m, I32, 0);
    let one = int(&mut m, I32, 1);
    let mut b = FunctionBuilder::new(&m, SymId::new(0), fn_ty).unwrap();
    let n = b.arg(0).unwrap();
    let [entry, header, latch, exit] = [(); 4].map(|()| b.create_block().unwrap());
    b.begin_block(entry).unwrap();
    b.br(header).unwrap();
    b.begin_block(header).unwrap();
    let next = b.reserve(I32).unwrap();
    assert!(b.is_reserved(next) && !b.is_reserved(n));
    assert_eq!(b.value_type(&m, next), I32);
    let i = b.phi(I32, &[(zero, entry), (next, latch)]).unwrap();
    let total = b.phi(I32, &[]).unwrap();
    let more = b.icmp(IntPredicate::Slt, I1, i, n).unwrap();
    b.cond_br(more, latch, exit).unwrap();
    b.begin_block(latch).unwrap();
    let bumped = b.binary(Opcode::Add, I32, i, one).unwrap();
    let sum = b.binary(Opcode::Add, I32, total, next).unwrap();
    b.resolve(next, bumped).unwrap();
    b.br(header).unwrap();
    b.begin_block(exit).unwrap();
    b.ret(Some(total)).unwrap();
    // The second phi's list arrives late and out of order with the
    // first's.
    b.add_incoming(total, sum, latch).unwrap();
    b.add_incoming(total, zero, entry).unwrap();
    assert_eq!(b.value_type(&m, i), I32);
    let body = b.finish().unwrap();
    body.verify(&m).unwrap();

    let header_block = body.block(header);
    assert!(header_block.has_phis());
    let phis: Vec<_> = header_block.phis().map(Inst::id).collect();
    assert_eq!(phis, [id(i), id(total)]);
    assert_eq!(header_block.non_phis().next().unwrap().id(), id(more));
    assert!(!body.block(entry).has_phis() && !body.block(latch).has_phis());
    let InstKind::Phi(list) = body.inst(id(i)).kind() else { panic!() };
    assert_eq!(list.len(), 2);
    assert!(!list.is_empty());
    assert_eq!(list.incoming().collect::<Vec<_>>(), [(zero, entry), (bumped, latch)]);
    assert_eq!(list.values(), [zero, bumped]);
    assert_eq!(list.blocks().collect::<Vec<_>>(), [entry, latch]);
    assert_eq!(body.inst(id(i)).operands(), [zero, bumped]);
    let InstKind::Phi(list) = body.inst(id(total)).kind() else { panic!() };
    assert_eq!(list.incoming().collect::<Vec<_>>(), [(sum, latch), (zero, entry)]);
    // The reserved operand of `sum` now names the definition.
    assert_eq!(body.inst(id(sum)).operands(), [total, bumped]);
    // `bumped`: one use by the phi (at the end of its own block) and one
    // by `sum`, which comes after it.
    let uses = body.uses(id(bumped));
    assert_eq!((uses.count(), uses.used_outside_block()), (2, false));
    assert!(!uses.only_use_is_next());
}

/// §5.3: a reserved value can be resolved to anything an operand can be,
/// before or after its uses; one that is never defined fails `finish`.
#[test]
fn reserved_values_resolve_or_fail() {
    let build = |how: u32| -> IrResult<(Module, Body, Value, Value)> {
        let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
        let fn_ty = m.types.function(I32, &[I32], false).unwrap();
        let five = int(&mut m, I32, 5);
        let mut b = FunctionBuilder::new(&m, SymId::new(0), fn_ty)?;
        let entry = b.create_block()?;
        b.begin_block(entry)?;
        let first = b.reserve(I32)?;
        let second = b.reserve(I32)?;
        let sum = b.binary(Opcode::Add, I32, first, second)?;
        b.ret(Some(sum))?;
        let target = match how {
            // To an argument, through a chain.
            0 => {
                b.resolve(first, second)?;
                b.resolve(second, b.arg(0)?)?;
                b.arg(0)?
            }
            // To a constant.
            1 => {
                b.resolve(first, five)?;
                b.resolve(second, five)?;
                five
            }
            // Never.
            2 => {
                b.resolve(first, five)?;
                five
            }
            // Only to each other.
            3 => {
                b.resolve(first, second)?;
                b.resolve(second, first)?;
                five
            }
            // To itself.
            _ => {
                b.resolve(first, first)?;
                b.resolve(second, five)?;
                five
            }
        };
        let body = b.finish()?;
        Ok((m, body, sum, target))
    };
    for how in [0, 1] {
        let (m, body, sum, target) = build(how).unwrap();
        body.verify(&m).unwrap();
        assert_eq!(body.inst(id(sum)).operands(), [target, target]);
        if how == 0 {
            assert_eq!(body.arg_uses(0).count(), 2);
            assert!(!body.arg_uses(0).used_outside_block());
        }
    }
    for how in [2, 3, 4] {
        assert_eq!(kind(build(how)), IrErrorKind::UndefinedValue, "case {how}");
    }

    // An unused reserved value must still be defined.
    let mut f = fx();
    f.b.reserve(I32).unwrap();
    f.b.ret(None).unwrap();
    assert_eq!(kind(f.b.finish()), IrErrorKind::UndefinedValue);

    // Misuse of `resolve`.
    let mut f = fx();
    let hole = f.b.reserve(I32).unwrap();
    assert_eq!(kind(f.b.resolve(f.a, f.a)), IrErrorKind::InvalidOperand);
    assert_eq!(kind(f.b.resolve(hole, Value::NONE)), IrErrorKind::InvalidOperand);
    f.b.resolve(hole, f.a).unwrap();
    assert_eq!(kind(f.b.resolve(hole, f.a)), IrErrorKind::InvalidOperand);
    drop(f.done());
}

/// §5.6: the three facts for instruction results, with every way a use
/// can be located.
#[test]
fn use_facts_of_instruction_results() {
    let mut f = fx();
    let b = &mut f.b;
    let [other, join] = [(); 2].map(|()| b.create_block().unwrap());
    let unused = b.binary(Opcode::Add, I32, f.a, f.a).unwrap();
    let next_only = b.binary(Opcode::Mul, I32, f.a, f.a).unwrap();
    let twice_by_next = b.binary(Opcode::Sub, I32, next_only, f.a).unwrap();
    let squared = b.binary(Opcode::Mul, I32, twice_by_next, twice_by_next).unwrap();
    let later = b.binary(Opcode::Xor, I32, squared, f.a).unwrap();
    let gap = b.binary(Opcode::Or, I32, f.a, f.a).unwrap();
    let after_gap = b.binary(Opcode::And, I32, later, f.a).unwrap();
    let escapes = b.binary(Opcode::Add, I32, gap, after_gap).unwrap();
    let to_phi = b.binary(Opcode::Add, I32, f.a, f.a).unwrap();
    let cond = b.icmp(IntPredicate::Eq, I1, to_phi, to_phi).unwrap();
    let branch = b.cond_br(cond, other, join).unwrap();
    b.begin_block(other).unwrap();
    let far = b.binary(Opcode::Add, I32, escapes, f.a).unwrap();
    let in_other = b.binary(Opcode::Add, I32, far, f.a).unwrap();
    b.br(join).unwrap();
    b.begin_block(join).unwrap();
    // `to_phi` arrives from the entry block, where it is defined;
    // `in_other` from `other`, where it is defined; `escapes` from
    // `other`, where it is not.
    let merged = b.phi(I32, &[(to_phi, f.entry), (in_other, other)]).unwrap();
    let second = b.phi(I32, &[(to_phi, f.entry), (escapes, other)]).unwrap();
    let first_use = b.binary(Opcode::Add, I32, merged, second).unwrap();
    b.ret(None).unwrap();
    let body = f.done();

    let facts = |value: Value| {
        let uses = body.uses(id(value));
        assert_eq!(uses, body.inst(id(value)).uses());
        (uses.count(), uses.used_outside_block(), uses.only_use_is_next())
    };
    assert_eq!(facts(unused), (0, false, false));
    assert!(body.uses(id(unused)).is_unused());
    assert_eq!(facts(next_only), (1, false, true));
    // One instruction, two operand positions: two uses.
    assert_eq!(facts(twice_by_next), (2, false, false));
    assert_eq!(facts(squared), (1, false, true));
    // A single use, in the same block, but not by the next instruction.
    assert_eq!(facts(later), (1, false, false));
    assert_eq!(facts(gap), (1, false, false));
    assert_eq!(facts(after_gap), (1, false, true));
    // Used in another block and as a phi input from another block.
    assert_eq!(facts(escapes), (2, true, false));
    // Two phi uses, both located at the end of the defining block, and
    // two by the instruction after it.
    assert_eq!(facts(to_phi), (4, false, false));
    // The condition's only use is the terminator right after it.
    assert_eq!(facts(cond), (1, false, true));
    assert_eq!(facts(branch), (0, false, false));
    assert_eq!(facts(far), (1, false, true));
    // Its only use is a phi in the next block: located in its own block,
    // and never "the next instruction", though the phi's id follows the
    // branch that follows it.
    assert_eq!(facts(in_other), (1, false, false));
    // A phi's own result is used like any other.
    assert_eq!(facts(merged), (1, false, false));
    assert_eq!(facts(second), (1, false, true));
    assert_eq!(facts(first_use), (0, false, false));
}

/// §5.6: where a use is located when the definition and the user are in
/// different blocks or the user is a phi, for operands given directly and
/// through reserved values.
#[test]
fn use_facts_across_a_block_boundary() {
    let facts = |body: &Body, value: Value| {
        let uses = body.uses(id(value));
        (uses.count(), uses.used_outside_block(), uses.only_use_is_next())
    };
    // An `invoke` result whose only user is the first instruction of the
    // block laid out next. That instruction has the next id but is in
    // another block: the use is outside, and not "by the next
    // instruction". If the user is a phi, the use is at the end of the
    // invoke's own block: not outside, and still not "next".
    for (through_reserved, by_phi) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut f = fx();
        let fn_ty = f.m.types.function(I32, &[], false).unwrap();
        let exn = f.m.types.struct_type(&[PTR, I32], false).unwrap();
        let b = &mut f.b;
        let [normal, pad] = [(); 2].map(|()| b.create_block().unwrap());
        let callee = Value::sym(SymId::new(2)).unwrap();
        let hole = b.reserve(I32).unwrap();
        let result = b.invoke(I32, &CallDesc::new(callee, fn_ty, &[]), normal, pad).unwrap();
        let operand = if through_reserved { hole } else { result };
        b.begin_block(normal).unwrap();
        let user = if by_phi {
            b.phi(I32, &[(operand, f.entry)]).unwrap()
        } else {
            b.freeze(I32, operand).unwrap()
        };
        b.resolve(hole, result).unwrap();
        b.ret(None).unwrap();
        b.begin_block(pad).unwrap();
        let caught = b.landing_pad(exn, true, &[]).unwrap();
        b.resume(caught).unwrap();
        let body = f.done();
        assert_eq!(id(user).raw(), id(result).raw() + 1);
        let case = format!("reserved: {through_reserved}, phi: {by_phi}");
        assert_eq!(facts(&body, result), (1, !by_phi, false), "{case}");
    }

    for through_reserved in [false, true] {
        // A phi whose only user is the next phi of its own block, on the
        // edge from that block to itself: located in the block, never
        // "next".
        let mut f = fx();
        let b = &mut f.b;
        let looping = b.create_block().unwrap();
        b.br(looping).unwrap();
        b.begin_block(looping).unwrap();
        let hole = b.reserve(I32).unwrap();
        let first = b.phi(I32, &[(f.a, f.entry), (f.a, looping)]).unwrap();
        let operand = if through_reserved { hole } else { first };
        let second = b.phi(I32, &[(f.a, f.entry), (operand, looping)]).unwrap();
        b.resolve(hole, first).unwrap();
        b.br(looping).unwrap();
        let body = f.done();
        assert_eq!(id(second).raw(), id(first).raw() + 1);
        assert_eq!(facts(&body, first), (1, false, false), "reserved: {through_reserved}");

        // A value whose only use is a phi input that arrives from a block
        // other than its own: used outside its block.
        let mut f = fx();
        let b = &mut f.b;
        let [other, join] = [(); 2].map(|()| b.create_block().unwrap());
        let hole = b.reserve(I32).unwrap();
        let def = b.binary(Opcode::Add, I32, f.a, f.a).unwrap();
        b.resolve(hole, def).unwrap();
        b.cond_br(f.c, other, join).unwrap();
        b.begin_block(other).unwrap();
        b.br(join).unwrap();
        b.begin_block(join).unwrap();
        let operand = if through_reserved { hole } else { def };
        // Listed for the edge from `other`; the entry block's edge
        // brings an argument.
        b.phi(I32, &[(f.a, f.entry), (operand, other)]).unwrap();
        let body = f.done();
        assert_eq!(facts(&body, def), (1, true, false), "reserved: {through_reserved}");
        // The argument: twice by `def` in the entry block, once by the
        // phi on the entry block's edge.
        assert_eq!((body.arg_uses(0).count(), body.arg_uses(0).used_outside_block()), (3, false));
    }
}

/// §5.6 (arguments): the count, and whether any use is outside the entry
/// block, with a phi use counted in its incoming block.
#[test]
fn use_facts_of_arguments() {
    let mut f = fx();
    let b = &mut f.b;
    let [other, join] = [(); 2].map(|()| b.create_block().unwrap());
    // a: twice in the entry block. l: once, in another block. x: only as
    // a phi input from the entry block. d: as a phi input from `other`.
    // p: by a call's argument and bundle. v, s: never.
    let void_ty = f.m.types.function(VOID, &[PTR], false).unwrap();
    b.binary(Opcode::Add, I32, f.a, f.a).unwrap();
    let inputs = [f.p];
    let args = [f.p];
    let desc = CallDesc {
        bundles: &[Bundle { tag: "x", inputs: &inputs }],
        ..CallDesc::new(f.p, void_ty, &args)
    };
    b.call(VOID, &desc).unwrap();
    b.cond_br(f.c, other, join).unwrap();
    b.begin_block(other).unwrap();
    b.binary(Opcode::Add, I64, f.l, f.l).unwrap();
    b.cast(Opcode::FPExt, DOUBLE, f.x).unwrap();
    b.br(join).unwrap();
    b.begin_block(join).unwrap();
    b.cast(Opcode::FPTrunc, FLOAT, f.d).unwrap();
    let body = f.done();
    let facts = |index: u32| {
        let uses = body.arg_uses(index);
        assert!(!uses.only_use_is_next(), "never reported for an argument");
        (uses.count(), uses.used_outside_block())
    };
    assert_eq!(facts(0), (2, false));
    assert_eq!(facts(1), (2, true));
    assert_eq!(facts(2), (1, true));
    assert_eq!(facts(3), (3, false), "callee, argument and bundle input");
    assert_eq!(facts(4), (0, false));
    assert_eq!(facts(5), (0, false));
    assert_eq!(facts(6), (1, true));
    assert_eq!(facts(7), (1, false));
    assert_eq!(body.arg_uses(99).count(), 0, "out of range reads as unused");

    // Phi inputs: located in the incoming block, which is the entry
    // block for one and not for the other.
    let mut f = fx();
    let b = &mut f.b;
    let [other, join] = [(); 2].map(|()| b.create_block().unwrap());
    b.cond_br(f.c, other, join).unwrap();
    b.begin_block(other).unwrap();
    b.br(join).unwrap();
    b.begin_block(join).unwrap();
    b.phi(I32, &[(f.a, f.entry), (f.a, other)]).unwrap();
    let late = b.phi(FLOAT, &[]).unwrap();
    b.add_incoming(late, f.x, f.entry).unwrap();
    b.add_incoming(late, f.x, f.entry).unwrap();
    b.ret(None).unwrap();
    let body = f.b.finish().unwrap();
    assert_eq!((body.arg_uses(0).count(), body.arg_uses(0).used_outside_block()), (2, true));
    assert_eq!((body.arg_uses(2).count(), body.arg_uses(2).used_outside_block()), (2, false));
    // The second phi lists the entry block twice for one edge.
    assert_eq!(kind(body.verify(&f.m)), IrErrorKind::BadBlock);
}

/// §5.6: uses that go through reserved values are recorded when the
/// value is resolved, with the same facts as direct uses.
#[test]
fn use_facts_through_reserved_values() {
    let mut f = fx();
    let b = &mut f.b;
    let [other, dead] = [(); 2].map(|()| b.create_block().unwrap());
    let early = b.reserve(I32).unwrap();
    let late = b.reserve(I32).unwrap();
    let from_arg = b.reserve(I32).unwrap();
    // Resolved before it is used: the user is the next instruction.
    let def = b.binary(Opcode::Add, I32, f.a, f.a).unwrap();
    b.resolve(early, def).unwrap();
    let user = b.binary(Opcode::Add, I32, early, from_arg).unwrap();
    b.br(other).unwrap();
    b.begin_block(other).unwrap();
    let late_def = b.binary(Opcode::Add, I32, user, user).unwrap();
    b.binary(Opcode::Add, I32, late, from_arg).unwrap();
    b.ret(None).unwrap();
    // Used before it is defined, in a block control does not reach
    // (where LLVM allows it, §5.8).
    b.begin_block(dead).unwrap();
    b.binary(Opcode::Add, I32, late, late).unwrap();
    b.resolve(late, late_def).unwrap();
    b.resolve(from_arg, f.a).unwrap();
    let body = f.done();
    let facts = |value: Value| {
        let uses = body.uses(id(value));
        (uses.count(), uses.used_outside_block(), uses.only_use_is_next())
    };
    assert_eq!(facts(def), (1, false, true));
    assert_eq!(body.inst(id(user)).operands(), [def, f.a]);
    // Two uses from the unreachable block, one by the instruction after
    // it.
    assert_eq!(facts(late_def), (3, true, false));
    // `a`: two direct uses in the entry block, one resolved use there
    // and one in the other block.
    assert_eq!((body.arg_uses(0).count(), body.arg_uses(0).used_outside_block()), (4, true));
}

/// §5.2: stack slots, their variables, and the dynamic alternative. §6:
/// the location array exists only when a location was set.
#[test]
fn stack_slots_and_locations() {
    let mut f = fx();
    let b = &mut f.b;
    let small = b.add_slot(4, 4).unwrap();
    let big = b.add_slot(1 << 33, 64).unwrap();
    let empty = b.add_slot(0, 1).unwrap();
    assert_eq!([small, big, empty], [SlotId::new(0), SlotId::new(1), SlotId::new(2)]);
    b.set_slot_var(big, DiId::new(11), LocId::new(3)).unwrap();
    b.set_slot_var(small, DiId::new(12), LocId::NONE).unwrap();
    let address = b.slot(small).unwrap();
    assert_eq!(address, Value::slot(small).unwrap());
    let first = b.store(f.a, address, align(4)).unwrap();
    b.set_loc(LocId::new(7));
    let second = b.load(I32, address, align(4)).unwrap();
    b.set_loc(LocId::NONE);
    let third = b.load(I32, address, align(4)).unwrap();
    b.set_loc(LocId::new(8));
    let body = f.done();
    let m = &f.m;

    assert_eq!(body.slots().len(), 3);
    assert_eq!((body.slots()[0].size(), body.slots()[0].align()), (4, 4));
    assert_eq!((body.slots()[1].size(), body.slots()[1].align_log2()), (1 << 33, 6));
    assert_eq!((body.slots()[2].size(), body.slots()[2].align()), (0, 1));
    let vars: Vec<_> = body.slot_vars().iter().map(|var| (var.slot, var.var, var.loc)).collect();
    assert_eq!(vars, [(big, DiId::new(11), LocId::new(3)), (small, DiId::new(12), LocId::NONE)]);
    assert_eq!(body.value_type(m, address), PTR);
    assert_eq!(address.as_slot(), Some(small));
    assert!(body.has_locs());
    assert_eq!(body.loc(id(first)), LocId::NONE, "appended before any location was set");
    assert_eq!(body.loc(id(second)), LocId::new(7));
    assert_eq!(body.inst(id(second)).loc(), LocId::new(7));
    assert_eq!(body.loc(id(third)), LocId::NONE);
    assert_eq!(body.block(f.entry).terminator().loc(), LocId::new(8));

    // No location set: no array.
    let mut f = fx();
    f.b.set_loc(LocId::NONE);
    let load = f.b.load(I32, f.p, align(1)).unwrap();
    let body = f.done();
    assert!(!body.has_locs());
    assert_eq!(body.loc(id(load)), LocId::NONE);

    let mut f = fx();
    assert_eq!(kind(f.b.add_slot(8, 3)), IrErrorKind::InvalidOperand);
    assert_eq!(kind(f.b.add_slot(8, 0)), IrErrorKind::InvalidOperand);
    assert_eq!(kind(f.b.slot(SlotId::new(0))), IrErrorKind::InvalidOperand);
    let unknown = SlotId::new(0);
    assert_eq!(
        kind(f.b.set_slot_var(unknown, DiId::new(0), LocId::NONE)),
        IrErrorKind::InvalidOperand
    );
    let missing = Value::slot(SlotId::new(5)).unwrap();
    assert_eq!(kind(f.b.load(I32, missing, align(1))), IrErrorKind::InvalidOperand);
}

/// §5.1: what the builder refuses as it goes. `BadBlock` for block
/// structure, `InvalidOperand` for an operand that names nothing.
#[test]
fn builder_errors() {
    // Not a function type.
    let m = Module::new(TargetSpec::aarch64_linux_gnu());
    assert_eq!(kind(FunctionBuilder::new(&m, SymId::new(0), I32)), IrErrorKind::InvalidType);

    // Blocks.
    let mut f = fx();
    let b = &mut f.b;
    let other = b.create_block().unwrap();
    assert_eq!(kind(b.begin_block(other)), IrErrorKind::BadBlock, "a block is open");
    assert_eq!(kind(b.begin_block(BlockId::new(77))), IrErrorKind::InvalidOperand);
    b.ret(None).unwrap();
    assert_eq!(kind(b.ret(None)), IrErrorKind::BadBlock, "after the terminator");
    assert_eq!(kind(b.binary(Opcode::Add, I32, f.a, f.a)), IrErrorKind::BadBlock);
    assert_eq!(kind(b.phi(I32, &[])), IrErrorKind::BadBlock);
    assert_eq!(kind(b.begin_block(f.entry)), IrErrorKind::BadBlock, "begun twice");
    b.begin_block(other).unwrap();
    let phi = b.phi(I32, &[]).unwrap();
    b.binary(Opcode::Add, I32, phi, f.a).unwrap();
    assert_eq!(kind(b.phi(I32, &[])), IrErrorKind::BadBlock, "a phi after a non-phi");
    assert_eq!(kind(f.b.finish()), IrErrorKind::BadBlock, "the last block is open");

    let mut f = fx();
    f.b.ret(None).unwrap();
    f.b.create_block().unwrap();
    assert_eq!(kind(f.b.finish()), IrErrorKind::BadBlock, "a block never begun");

    let m = Module::new(TargetSpec::aarch64_linux_gnu());
    let mut types = Module::new(TargetSpec::aarch64_linux_gnu());
    let fn_ty = types.types.function(VOID, &[], false).unwrap();
    drop(m);
    let b = FunctionBuilder::new(&types, SymId::new(0), fn_ty).unwrap();
    assert_eq!(kind(b.finish()), IrErrorKind::BadBlock, "no block at all");

    // Operands.
    let mut f = fx();
    let void_ty = f.m.types.function(VOID, &[], false).unwrap();
    let b = &mut f.b;
    let future = Value::inst(InstId::new(40)).unwrap();
    let asm = Value::asm(AsmId::new(0)).unwrap();
    let bad_arg = Value::arg(8).unwrap();
    for bad in [future, asm, bad_arg, Value::NONE] {
        assert_eq!(kind(b.binary(Opcode::Add, I32, f.a, bad)), IrErrorKind::InvalidOperand);
        assert_eq!(kind(b.store(bad, f.p, align(1))), IrErrorKind::InvalidOperand);
        assert_eq!(kind(b.ret(Some(bad))), IrErrorKind::InvalidOperand);
        assert_eq!(kind(b.ptr_add(PTR, f.p, 0, &[(bad, 1)])), IrErrorKind::InvalidOperand);
        assert_eq!(kind(b.switch(bad, f.entry, &[])), IrErrorKind::InvalidOperand);
        assert_eq!(kind(b.indirect_br(bad, &[])), IrErrorKind::InvalidOperand);
        assert_eq!(kind(b.shuffle_vector(f.v4, f.v, bad, &[])), IrErrorKind::InvalidOperand);
        let args = [bad];
        let desc = CallDesc::new(f.p, void_ty, &args);
        assert_eq!(kind(b.call(VOID, &desc)), IrErrorKind::InvalidOperand);
        let bundles = [Bundle { tag: "t", inputs: &args }];
        let desc = CallDesc { bundles: &bundles, ..CallDesc::new(f.p, void_ty, &[]) };
        assert_eq!(kind(b.call(VOID, &desc)), IrErrorKind::InvalidOperand);
        assert_eq!(kind(b.phi(I32, &[(bad, f.entry)])), IrErrorKind::InvalidOperand);
    }
    assert_eq!(kind(b.arg(8)), IrErrorKind::InvalidOperand);
    let none = CallDesc::new(Value::NONE, void_ty, &[]);
    assert_eq!(kind(b.call(VOID, &none)), IrErrorKind::InvalidOperand);
    // Opcode classes.
    assert_eq!(kind(b.binary(Opcode::ICmp, I32, f.a, f.a)), IrErrorKind::InvalidOperand);
    assert_eq!(kind(b.cast(Opcode::Add, I32, f.a)), IrErrorKind::InvalidOperand);
    assert_eq!(kind(b.funclet(Opcode::Ret, VOID)), IrErrorKind::InvalidOperand);
    // Blocks that were not created.
    let nowhere = BlockId::new(9);
    assert_eq!(kind(b.br(nowhere)), IrErrorKind::InvalidOperand);
    assert_eq!(kind(b.cond_br(f.c, f.entry, nowhere)), IrErrorKind::InvalidOperand);
    assert_eq!(kind(b.switch(f.a, nowhere, &[])), IrErrorKind::InvalidOperand);
    assert_eq!(kind(b.indirect_br(f.p, &[nowhere])), IrErrorKind::InvalidOperand);
    assert_eq!(kind(b.phi(I32, &[(f.a, nowhere)])), IrErrorKind::InvalidOperand);
    let desc = CallDesc::new(f.p, void_ty, &[]);
    assert_eq!(kind(b.invoke(VOID, &desc, f.entry, nowhere)), IrErrorKind::InvalidOperand);
    assert_eq!(kind(b.callbr(VOID, &desc, f.entry, &[nowhere])), IrErrorKind::InvalidOperand);
    // Calls.
    let overloads = [I32];
    let with_overloads = CallDesc { overloads: &overloads, ..desc };
    assert_eq!(kind(b.call(VOID, &with_overloads)), IrErrorKind::InvalidOperand);
    let short = CallDesc::intrinsic(Intrinsic::Memcpy, &overloads, void_ty, &[]);
    assert_eq!(kind(b.call(VOID, &short)), IrErrorKind::InvalidOperand);
    let named = CallDesc::intrinsic(Intrinsic::Other("llvm.x"), &overloads, void_ty, &[]);
    assert_eq!(kind(b.call(VOID, &named)), IrErrorKind::InvalidOperand);
    let attrs = [ParamAttrs::NONE];
    let uneven = CallDesc { arg_attrs: &attrs, ..desc };
    assert_eq!(kind(b.call(VOID, &uneven)), IrErrorKind::InvalidOperand);
    // Alignment, offsets, phis.
    assert_eq!(kind(b.alloca(PTR, f.l, 1, 24)), IrErrorKind::InvalidOperand);
    assert_eq!(kind(b.insert_value(f.st, f.s, f.a, 1 << 32)), IrErrorKind::TypeTooLarge);
    assert_eq!(kind(b.add_incoming(f.a, f.a, f.entry)), IrErrorKind::InvalidOperand);
    let add = b.binary(Opcode::Add, I32, f.a, f.a).unwrap();
    assert_eq!(kind(b.add_incoming(add, f.a, f.entry)), IrErrorKind::InvalidOperand);

    // None of the refused appends left anything behind.
    assert_eq!(b.inst_count(), 1);
    let body = f.done();
    let m = &f.m;
    assert_eq!(body.inst_count(), 2);
    assert_eq!(body.call_count(), 0);
    assert_eq!(body.arg_uses(0).count(), 2);
    assert!((1..8).all(|index| body.arg_uses(index).is_unused()));
    body.verify(m).unwrap();
}

/// §5.1: a body's storage can be handed to the next builder; the result
/// is the same body a fresh builder makes, and once the tables have
/// grown, building allocates nothing more.
#[test]
fn recycling_storage() {
    fn build(seed: u32, storage: Option<Body>) -> (Module, Body) {
        let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
        let fn_ty = m.types.function(I32, &[I32, PTR], true).unwrap();
        let callee_ty = m.types.function(I32, &[I32], false).unwrap();
        let exn = m.types.struct_type(&[PTR, I32], false).unwrap();
        let one = m.consts.int(&m.types, I32, 1).unwrap();
        let sym = SymId::new(seed);
        let mut b = match storage {
            Some(storage) => FunctionBuilder::reusing(&m, sym, fn_ty, storage).unwrap(),
            None => FunctionBuilder::new(&m, sym, fn_ty).unwrap(),
        };
        let (a, p) = (b.arg(0).unwrap(), b.arg(1).unwrap());
        let [entry, body_block, pad, exit] = [(); 4].map(|()| b.create_block().unwrap());
        let slot = b.add_slot(8, 8).unwrap();
        b.set_slot_var(slot, DiId::new(seed), LocId::new(seed)).unwrap();
        let address = b.slot(slot).unwrap();
        b.begin_block(entry).unwrap();
        b.set_loc(LocId::new(1));
        b.store(a, address, align(4)).unwrap();
        b.br(body_block).unwrap();
        b.begin_block(body_block).unwrap();
        let hole = b.reserve(I32).unwrap();
        let phi = b.phi(I32, &[(a, entry), (hole, exit)]).unwrap();
        let mut value = phi;
        for step in 0..seed {
            let moved = b.ptr_add(PTR, p, i64::from(step), &[(value, 4)]).unwrap();
            value = b.load(I32, moved, align(4)).unwrap();
            let args = [value];
            let inputs = [value];
            let bundles = [Bundle { tag: "deopt", inputs: &inputs }];
            let desc = CallDesc {
                bundles: &bundles,
                ret_attrs: ParamAttrs::NONE.with(ParamAttrs::ZEROEXT).unwrap(),
                ..CallDesc::new(p, callee_ty, &args)
            };
            value = b.call(I32, &desc).unwrap();
        }
        b.resolve(hole, value).unwrap();
        // An intrinsic hod does not know that LLVM lets `invoke` call.
        let unknown = Intrinsic::Other("llvm.experimental.patchpoint.i32");
        let void_ty_args = [value];
        let desc = CallDesc::intrinsic(unknown, &[], callee_ty, &void_ty_args);
        let last = b.invoke(I32, &desc, exit, pad).unwrap();
        b.begin_block(pad).unwrap();
        let caught = b.landing_pad(exn, true, &[]).unwrap();
        b.resume(caught).unwrap();
        b.begin_block(exit).unwrap();
        b.switch(last, body_block, &[(one, exit)]).unwrap();
        let body = b.finish().unwrap();
        (m, body)
    }

    // A large body, then a small one in its storage, then the large one
    // again: each equals the freshly built one.
    let (m_large, large) = build(40, None);
    large.verify(&m_large).unwrap();
    let large_dump = dump(&large, &m_large);
    let allocated = large.allocated_bytes();
    assert!(allocated >= large.table_bytes() && large.table_bytes() > 40 * 3 * 20);

    let (m_small, fresh_small) = build(2, None);
    let (_, small) = build(2, Some(large));
    assert_eq!(dump(&small, &m_small), dump(&fresh_small, &m_small));
    assert_eq!(small.allocated_bytes(), allocated, "nothing was allocated or freed");
    assert!(small.table_bytes() < fresh_small.allocated_bytes() + 1);

    let (_, again) = build(40, Some(small));
    assert_eq!(dump(&again, &m_large), large_dump);
    assert_eq!(again.allocated_bytes(), allocated, "the steady state allocates nothing");

    // An abandoned builder gives its storage back, empty.
    let fn_ty = again.fn_type();
    let mut b = FunctionBuilder::reusing(&m_large, SymId::new(0), fn_ty, again).unwrap();
    let block = b.create_block().unwrap();
    b.begin_block(block).unwrap();
    b.unreachable().unwrap();
    let storage = b.abandon();
    assert_eq!((storage.inst_count(), storage.block_count()), (0, 0));
    assert_eq!(storage.allocated_bytes(), allocated);
    assert_eq!(storage.table_bytes(), 0);
    // An empty body is not a function body.
    assert!(storage.verify(&m_large).is_err());
}

/// §5.1, §5.4: the type rules `verify` enforces, one ill-typed body per
/// rule, each beside a well-typed neighbour.
#[test]
#[allow(clippy::too_many_lines)]
fn verify_type_rules() {
    use IrErrorKind::{BadIndex, InvalidOperand, InvalidType, TypeMismatch, UnsizedType};
    let ok = None;
    let no = Some;
    let seq = align(4).with_atomic(AtomicOrdering::SeqCst, SyncScope::System);

    // Binary operations.
    assert_eq!(verdict(|f| drop(f.b.binary(Opcode::Add, I32, f.a, f.a))), ok);
    assert_eq!(verdict(|f| drop(f.b.binary(Opcode::Add, f.v4, f.v, f.v))), ok);
    assert_eq!(verdict(|f| drop(f.b.binary(Opcode::Add, I32, f.a, f.l))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.binary(Opcode::Add, I64, f.a, f.a))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.binary(Opcode::Add, FLOAT, f.x, f.x))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.binary(Opcode::FAdd, I32, f.a, f.a))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.binary(Opcode::Add, PTR, f.p, f.p))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.fneg(I32, f.a))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.fneg(FLOAT, f.d))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.freeze(I64, f.a))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.freeze(f.st, f.s))), ok);

    // Comparisons.
    assert_eq!(verdict(|f| drop(f.b.icmp(IntPredicate::Eq, I1, f.p, f.p))), ok);
    assert_eq!(verdict(|f| drop(f.b.icmp(IntPredicate::Eq, I1, f.a, f.l))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.icmp(IntPredicate::Eq, I32, f.a, f.a))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.icmp(IntPredicate::Eq, I1, f.v, f.v))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.icmp(IntPredicate::Eq, I1, f.x, f.x))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.fcmp(FloatPredicate::Oeq, I1, f.a, f.a))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.fcmp(FloatPredicate::Oeq, I1, f.d, f.d))), ok);

    // Casts (the full grid is checked against LLVM in `body_llvm`).
    assert_eq!(verdict(|f| drop(f.b.cast(Opcode::Trunc, I64, f.a))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.cast(Opcode::ZExt, I32, f.a))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.cast(Opcode::BitCast, I64, f.a))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.cast(Opcode::BitCast, PTR, f.l))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.cast(Opcode::BitCast, I64, f.d))), ok);
    assert_eq!(verdict(|f| drop(f.b.cast(Opcode::AddrSpaceCast, PTR, f.p))), no(InvalidType));
    // `ptrtoaddr` gives exactly the address width; `ptrtoint` any width.
    assert_eq!(verdict(|f| drop(f.b.cast(Opcode::PtrToAddr, I64, f.p))), ok);
    assert_eq!(verdict(|f| drop(f.b.cast(Opcode::PtrToAddr, I32, f.p))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.cast(Opcode::PtrToInt, I32, f.p))), ok);

    // Memory.
    assert_eq!(verdict(|f| drop(f.b.load(I32, f.a, align(4)))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.load(VOID, f.p, align(4)))), no(UnsizedType));
    assert_eq!(verdict(|f| drop(f.b.load(f.st, f.p, align(8)))), ok);
    assert_eq!(verdict(|f| drop(f.b.store(f.a, f.l, align(4)))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.load(I32, f.p, seq))), ok);
    assert_eq!(verdict(|f| drop(f.b.load(f.st, f.p, seq))), no(InvalidType));
    let release = align(4).with_atomic(AtomicOrdering::Release, SyncScope::System);
    let acquire = align(4).with_atomic(AtomicOrdering::Acquire, SyncScope::System);
    assert_eq!(verdict(|f| drop(f.b.load(I32, f.p, release))), no(InvalidOperand));
    assert_eq!(verdict(|f| drop(f.b.store(f.a, f.p, acquire))), no(InvalidOperand));
    assert_eq!(verdict(|f| drop(f.b.store(f.a, f.p, release))), ok);
    assert_eq!(verdict(|f| drop(f.b.alloca(PTR, f.x, 4, 4))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.alloca(I64, f.a, 4, 4))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.alloca(PTR, f.a, 4, 4))), ok);
    assert_eq!(verdict(|f| drop(f.b.fence(AtomicOrdering::Acquire, SyncScope::System))), ok);
    assert_eq!(
        verdict(|f| drop(f.b.fence(AtomicOrdering::Monotonic, SyncScope::System))),
        no(InvalidOperand)
    );
    let mono = AtomicOrdering::Monotonic;
    let cas = |elem: TypeId, pick: fn(&Fx) -> (Value, Value), mem: MemFlags, failure| {
        verdict(|f| {
            let result = f.m.types.struct_type(&[elem, I1], false).unwrap();
            let (expected, replacement) = pick(f);
            drop(f.b.cmpxchg(result, f.p, expected, replacement, mem, failure, false));
        })
    };
    assert_eq!(cas(I32, |f| (f.a, f.a), seq, mono), ok);
    assert_eq!(cas(PTR, |f| (f.p, f.p), seq, mono), ok);
    assert_eq!(cas(I64, |f| (f.a, f.a), seq, mono), no(TypeMismatch));
    assert_eq!(cas(FLOAT, |f| (f.x, f.x), seq, mono), no(InvalidType));
    assert_eq!(cas(I32, |f| (f.a, f.a), align(4), mono), no(InvalidOperand));
    assert_eq!(cas(I32, |f| (f.a, f.a), seq, AtomicOrdering::Release), no(InvalidOperand));
    assert_eq!(cas(I32, |f| (f.a, f.l), seq, mono), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.atomic_rmw(RmwOp::Add, I32, f.p, f.a, seq))), ok);
    assert_eq!(verdict(|f| drop(f.b.atomic_rmw(RmwOp::Add, PTR, f.p, f.p, seq))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.atomic_rmw(RmwOp::FAdd, I32, f.p, f.a, seq))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.atomic_rmw(RmwOp::Add, I64, f.p, f.a, seq))), no(TypeMismatch));
    assert_eq!(
        verdict(|f| drop(f.b.atomic_rmw(RmwOp::Add, I32, f.p, f.a, align(4)))),
        no(InvalidOperand)
    );

    // Addresses and aggregates.
    assert_eq!(verdict(|f| drop(f.b.ptr_add(PTR, f.l, 0, &[]))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.ptr_add(I64, f.p, 0, &[]))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.ptr_add(PTR, f.p, 0, &[(f.x, 1)]))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.ptr_add(PTR, f.p, 0, &[(f.v, 1)]))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.extract_value(I64, f.s, 8))), ok);
    assert_eq!(verdict(|f| drop(f.b.extract_value(I32, f.s, 8))), no(BadIndex));
    assert_eq!(verdict(|f| drop(f.b.extract_value(I32, f.s, 4))), no(BadIndex));
    assert_eq!(verdict(|f| drop(f.b.extract_value(I32, f.v, 0))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.insert_value(f.st, f.s, f.l, 8))), ok);
    assert_eq!(verdict(|f| drop(f.b.insert_value(f.st, f.s, f.a, 8))), no(BadIndex));
    assert_eq!(verdict(|f| drop(f.b.insert_value(f.v4, f.s, f.l, 8))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.extract_element(I32, f.s, f.a))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.extract_element(I64, f.v, f.a))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.extract_element(I32, f.v, f.x))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.insert_element(f.v4, f.v, f.l, f.a))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.shuffle_vector(f.v4, f.v, f.v, &[0, 7, 1, 2]))), ok);
    assert_eq!(verdict(|f| drop(f.b.shuffle_vector(f.v4, f.v, f.v, &[0, 8, 1, 2]))), no(BadIndex));
    assert_eq!(verdict(|f| drop(f.b.shuffle_vector(f.v4, f.v, f.v, &[0, 1]))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.shuffle_vector(f.v4, f.a, f.a, &[0; 4]))), no(InvalidType));

    // Select, va_arg, returns.
    assert_eq!(verdict(|f| drop(f.b.select(I32, f.c, f.a, f.a))), ok);
    assert_eq!(verdict(|f| drop(f.b.select(I32, f.a, f.a, f.a))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.select(I32, f.c, f.a, f.l))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.va_arg(I32, f.a))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.va_arg(VOID, f.p))), no(UnsizedType));
    assert_eq!(verdict(|f| drop(f.b.ret(Some(f.a)))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.cond_br(f.a, f.entry, f.entry))), no(TypeMismatch));
    assert_eq!(verdict(|f| drop(f.b.indirect_br(f.a, &[]))), no(InvalidType));
    assert_eq!(verdict(|f| drop(f.b.switch(f.x, f.entry, &[]))), no(InvalidType));
    // An instruction without a result is not a value.
    assert_eq!(
        verdict(|f| {
            let store = f.b.store(f.a, f.p, align(4)).unwrap();
            drop(f.b.freeze(VOID, store));
        }),
        no(InvalidOperand)
    );
    // A type id from nowhere.
    assert_eq!(verdict(|f| drop(f.b.load(TypeId::new(10_000), f.p, align(4)))), no(InvalidOperand));
    // A constant from nowhere.
    let stray = Value::constant(hod_ir::ConstId::new(10_000)).unwrap();
    assert_eq!(verdict(|f| drop(f.b.store(stray, f.p, align(4)))), no(InvalidOperand));
}

/// §5.4, §5.5: the rules for calls, landing pads, switches and phis
/// that `verify` enforces.
#[test]
#[allow(clippy::too_many_lines)]
fn verify_call_and_control_rules() {
    use IrErrorKind::{BadBlock, InvalidOperand, InvalidType, TypeMismatch};
    let callee = Value::sym(SymId::new(3)).unwrap();
    // The argument values are the same in every fixture.
    let g = fx();
    let call = |f: &mut Fx, ret: TypeId, params: &[TypeId], variadic: bool, args: &[Value]| {
        let fn_ty = f.m.types.function(ret, params, variadic).unwrap();
        f.b.call(ret, &CallDesc::new(callee, fn_ty, args)).unwrap();
    };
    assert_eq!(verdict(|f| call(f, I32, &[I32], false, &[g.a])), None);
    assert_eq!(verdict(|f| call(f, I32, &[I32], true, &[g.a, g.x, g.p])), None);
    assert_eq!(verdict(|f| call(f, I32, &[I32], false, &[g.l])), Some(TypeMismatch));
    assert_eq!(verdict(|f| call(f, I32, &[I32], false, &[])), Some(InvalidOperand));
    assert_eq!(verdict(|f| call(f, I32, &[I32], false, &[g.a, g.a])), Some(InvalidOperand));
    assert_eq!(verdict(|f| call(f, I32, &[I32], true, &[])), Some(InvalidOperand));
    // The result type is the function type's.
    assert_eq!(
        verdict(|f| {
            let fn_ty = f.m.types.function(I32, &[], false).unwrap();
            drop(f.b.call(I64, &CallDesc::new(callee, fn_ty, &[])));
        }),
        Some(TypeMismatch)
    );
    // The function type is one; the callee is a pointer.
    assert_eq!(
        verdict(|f| drop(f.b.call(I32, &CallDesc::new(callee, I32, &[])))),
        Some(InvalidType)
    );
    assert_eq!(
        verdict(|f| {
            let fn_ty = f.m.types.function(VOID, &[], false).unwrap();
            drop(f.b.call(VOID, &CallDesc::new(f.a, fn_ty, &[])));
        }),
        Some(InvalidType)
    );
    // An intrinsic's overload types are the ones its signature gives.
    let memset = |f: &mut Fx, overloads: &[TypeId]| {
        let fn_ty = f.m.types.function(VOID, &[PTR, I8, I64, I1], false).unwrap();
        let byte = int(&mut f.m, I8, 0);
        let flag = int(&mut f.m, I1, 0);
        let args = [f.p, byte, f.l, flag];
        drop(f.b.call(VOID, &CallDesc::intrinsic(Intrinsic::Memset, overloads, fn_ty, &args)));
    };
    assert_eq!(verdict(|f| memset(f, &[PTR, I64])), None);
    assert_eq!(verdict(|f| memset(f, &[PTR, I32])), Some(TypeMismatch));
    assert_eq!(
        verdict(|f| {
            let fn_ty = f.m.types.function(VOID, &[], false).unwrap();
            drop(f.b.call(VOID, &CallDesc::intrinsic(Intrinsic::Other("abort"), &[], fn_ty, &[])));
        }),
        Some(InvalidOperand)
    );

    // Landing pads.
    let invoke = |f: &mut Fx, pad_first: bool, branch_to_pad: bool, clause: Option<Clause>| {
        let fn_ty = f.m.types.function(VOID, &[], false).unwrap();
        let exn = f.m.types.struct_type(&[PTR, I32], false).unwrap();
        let [normal, pad] = [(); 2].map(|()| f.b.create_block().unwrap());
        f.b.invoke(VOID, &CallDesc::new(callee, fn_ty, &[]), normal, pad).unwrap();
        f.b.begin_block(normal).unwrap();
        if branch_to_pad {
            f.b.br(pad).unwrap();
        } else {
            f.b.ret(None).unwrap();
        }
        f.b.begin_block(pad).unwrap();
        if !pad_first {
            f.b.binary(Opcode::Add, I32, f.a, f.a).unwrap();
        }
        let clauses: Vec<Clause> = clause.into_iter().collect();
        let caught = f.b.landing_pad(exn, clauses.is_empty(), &clauses).unwrap();
        f.b.resume(caught).unwrap();
    };
    assert_eq!(verdict(|f| invoke(f, true, false, None)), None);
    assert_eq!(verdict(|f| invoke(f, false, false, None)), Some(BadBlock));
    assert_eq!(verdict(|f| invoke(f, true, true, None)), Some(BadBlock));
    let clause = |f: &mut Fx, ty: TypeId, filter: bool| {
        let constant = f.m.consts.zero(&f.m.types, ty).unwrap();
        if filter { Clause::Filter(constant) } else { Clause::Catch(constant) }
    };
    let with_clause = |ty: TypeId, filter: bool| {
        verdict(|f| {
            let clause = clause(f, ty, filter);
            invoke(f, true, false, Some(clause));
        })
    };
    assert_eq!(with_clause(PTR, false), None);
    assert_eq!(with_clause(I32, false), Some(InvalidType));
    assert_eq!(with_clause(PTR, true), Some(InvalidType));
    // An invoke must unwind to a pad.
    assert_eq!(
        verdict(|f| {
            let fn_ty = f.m.types.function(VOID, &[], false).unwrap();
            let [normal, other] = [(); 2].map(|()| f.b.create_block().unwrap());
            f.b.invoke(VOID, &CallDesc::new(callee, fn_ty, &[]), normal, other).unwrap();
            f.b.begin_block(normal).unwrap();
            f.b.ret(None).unwrap();
            f.b.begin_block(other).unwrap();
        }),
        Some(BadBlock)
    );
    // `resume` takes what the pads make.
    assert_eq!(
        verdict(|f| {
            invoke(f, true, false, None);
            let block = f.b.create_block().unwrap();
            f.b.begin_block(block).unwrap();
            f.b.resume(f.a).unwrap();
        }),
        Some(TypeMismatch)
    );

    // Switch cases are distinct literal integers of the condition's type.
    let switch = |f: &mut Fx, cases: &[(TypeId, u64)]| {
        let target = f.b.create_block().unwrap();
        let cases: Vec<_> = cases
            .iter()
            .map(|&(ty, value)| (f.m.consts.int(&f.m.types, ty, value).unwrap(), target))
            .collect();
        f.b.switch(f.a, target, &cases).unwrap();
        f.b.begin_block(target).unwrap();
    };
    assert_eq!(verdict(|f| switch(f, &[(I32, 1), (I32, 2)])), None);
    assert_eq!(verdict(|f| switch(f, &[(I32, 1), (I64, 2)])), Some(TypeMismatch));
    assert_eq!(verdict(|f| switch(f, &[(I32, 1), (I32, 1)])), Some(InvalidOperand));
    assert_eq!(
        verdict(|f| {
            let target = f.b.create_block().unwrap();
            let undef = f.m.consts.undef(&f.m.types, I32).unwrap();
            f.b.switch(f.a, target, &[(undef, target)]).unwrap();
            f.b.begin_block(target).unwrap();
        }),
        Some(InvalidOperand)
    );

    // Phis: one entry per incoming edge, the same value on parallel
    // edges, the block's type, never in the entry block.
    let diamond = |f: &mut Fx, incoming: &Incoming| {
        let [left, join] = [(); 2].map(|()| f.b.create_block().unwrap());
        let one = f.m.consts.int(&f.m.types, I32, 1).unwrap();
        let two = f.m.consts.int(&f.m.types, I32, 2).unwrap();
        f.b.switch(f.a, join, &[(one, left), (two, join)]).unwrap();
        f.b.begin_block(left).unwrap();
        f.b.br(join).unwrap();
        f.b.begin_block(join).unwrap();
        let list = incoming(f, f.entry, left);
        f.b.phi(I32, &list).unwrap();
    };
    assert_eq!(verdict(|f| diamond(f, &|f, e, l| vec![(f.a, e), (f.a, e), (f.a, l)])), None);
    assert_eq!(verdict(|f| diamond(f, &|f, e, l| vec![(f.a, e), (f.a, l)])), Some(BadBlock));
    assert_eq!(verdict(|f| diamond(f, &|f, e, _| vec![(f.a, e), (f.a, e)])), Some(BadBlock));
    assert_eq!(
        verdict(|f| diamond(f, &|f, e, l| vec![(f.a, e), (f.a, e), (f.a, l), (f.a, l)])),
        Some(BadBlock)
    );
    assert_eq!(
        verdict(|f| diamond(f, &|f, e, l| vec![(f.a, e), (f.l, e), (f.a, l)])),
        Some(TypeMismatch)
    );
    assert_eq!(
        verdict(|f| {
            let zero = int(&mut f.m, I32, 0);
            diamond(f, &move |f, e, l| vec![(f.a, e), (zero, e), (f.a, l)]);
        }),
        Some(InvalidOperand)
    );
    assert_eq!(verdict(|f| drop(f.b.phi(I32, &[]))), Some(BadBlock), "a phi in the entry block");
    // The entry block has no predecessors.
    assert_eq!(verdict(|f| drop(f.b.br(f.entry))), Some(BadBlock));
}
