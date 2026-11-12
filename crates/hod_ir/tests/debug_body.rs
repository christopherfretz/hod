//! L0 tests of debug information in function bodies (`spec/ir.md` §6.6,
//! §6.8): variables in stack slots, declarations of other addresses,
//! value records, labels and locations are built with `FunctionBuilder`
//! and read back through the API code generation uses, and `Body::verify`
//! accepts the well-formed bodies and names the rule each ill-formed one
//! breaks.

use hod_ir::{
    AsmId, Body, DbgDeclare, DbgRecordKind, DiBasicType, DiCompileUnit, DiFile, DiId, DiLabel,
    DiLexicalBlock, DiLocalVariable, DiLocation, DiOp, DiSubprogram, EmissionKind, ExprId,
    FunctionBuilder, FunctionDesc, InstId, IrErrorKind, IrResult, LocId, MemFlags, Module, Opcode,
    SlotVar, SpFlags, SymId, TargetSpec, TypeId, Value,
};

const I32: TypeId = TypeId::I32;
const PTR: TypeId = TypeId::PTR;

fn kind<T: std::fmt::Debug>(result: IrResult<T>) -> IrErrorKind {
    result.unwrap_err().kind
}

/// A module with one compile unit and the functions `f` (with a symbol
/// and the body under construction), `g` and `h` (which get inlined), and
/// a builder for `i32 f(i32, ptr)` with its entry block open.
struct Fx {
    m: Module,
    b: FunctionBuilder,
    sym: SymId,
    fn_ty: TypeId,
    file: DiId,
    int: DiId,
    f: DiId,
    g: DiId,
    h: DiId,
    /// The first line of `f`.
    at: LocId,
}

fn fx() -> Fx {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let d = &mut m.debug;
    let file = d.add(&DiFile { filename: b"a.c", ..Default::default() }.into()).unwrap();
    let unit = DiCompileUnit {
        language: 0x1d,
        file: Some(file),
        emission_kind: EmissionKind::FullDebug,
        ..Default::default()
    };
    let unit = d.add_distinct(&unit.into()).unwrap();
    d.add_compile_unit(unit).unwrap();
    let int = DiBasicType { name: b"int", size: 32, encoding: 5, ..Default::default() };
    let int = d.add(&int.into()).unwrap();
    let mut function = |name| {
        let node = DiSubprogram {
            name,
            file: Some(file),
            sp_flags: SpFlags::DEFINITION,
            unit: Some(unit),
            ..Default::default()
        };
        d.add_distinct(&node.into()).unwrap()
    };
    let (f, g, h) = (function(b"f"), function(b"g"), function(b"h"));
    let at = d.loc(DiLocation { line: 1, column: 1, scope: f, inlined_at: LocId::NONE }).unwrap();
    let fn_ty = m.types.function(I32, &[I32, PTR], false).unwrap();
    let sym = m.symbol_named(b"f").unwrap();
    let desc = FunctionDesc { subprogram: Some(f), has_body: true, ..FunctionDesc::new(fn_ty) };
    m.set_function(sym, &desc).unwrap();
    let mut b = FunctionBuilder::new(&m, sym, fn_ty).unwrap();
    let entry = b.create_block().unwrap();
    b.begin_block(entry).unwrap();
    Fx { m, b, sym, fn_ty, file, int, f, g, h, at }
}

impl Fx {
    /// A local variable of `scope`.
    fn var(&mut self, name: &[u8], scope: DiId) -> DiId {
        let node = DiLocalVariable {
            name,
            scope: Some(scope),
            file: Some(self.file),
            ty: Some(self.int),
            ..Default::default()
        };
        self.m.debug.add(&node.into()).unwrap()
    }

    fn loc(&mut self, line: u32, scope: DiId, inlined_at: LocId) -> LocId {
        self.m.debug.loc(DiLocation { line, column: 1, scope, inlined_at }).unwrap()
    }

    /// Ends the open block with `ret i32 %0`-like code and finishes.
    fn finish(&mut self) -> Body {
        if self.b.current_block().is_some() {
            let value = self.b.arg(0).unwrap();
            self.b.ret(Some(value)).unwrap();
        }
        let empty = FunctionBuilder::new(&self.m, self.sym, self.fn_ty).unwrap();
        std::mem::replace(&mut self.b, empty).finish().unwrap()
    }

    /// [`Fx::finish`], and the tables and the body verify.
    fn done(&mut self) -> Body {
        let body = self.finish();
        self.m.finish_debug().unwrap();
        self.m.verify_debug().unwrap();
        body.verify(&self.m).unwrap();
        body
    }

    /// What `verify` says of the body.
    fn verdict(&mut self) -> Option<IrErrorKind> {
        let body = self.finish();
        self.m.verify_debug().unwrap();
        body.verify(&self.m).err().map(|error| error.kind)
    }
}

fn align(bytes: u64) -> MemFlags {
    MemFlags::aligned(bytes).unwrap()
}

/// §6.6: a slot holds several variables: the pieces of one, each with a
/// fragment expression, and a variable of a function inlined into this
/// one, told apart by its location.
#[test]
fn several_variables_in_one_slot() {
    let mut f = fx();
    let (whole, pair, inlined) = (f.var(b"whole", f.f), f.var(b"pair", f.f), f.var(b"x", f.g));
    let low = f.m.debug.expr(&[DiOp::Fragment(0, 32)]).unwrap();
    let high = f.m.debug.expr(&[DiOp::PlusUconst(4), DiOp::Fragment(32, 32)]).unwrap();
    let call = f.loc(7, f.f, LocId::NONE);
    let in_g = f.loc(20, f.g, call);
    let (at, b) = (f.at, &mut f.b);
    let slot = b.add_slot(8, 4).unwrap();
    let other = b.add_slot(4, 4).unwrap();
    let address = b.slot(slot).unwrap();
    b.dbg_declare(address, pair, low, at).unwrap();
    b.dbg_declare(address, pair, high, at).unwrap();
    b.set_slot_var(other, whole, at).unwrap();
    b.dbg_declare(address, inlined, ExprId::EMPTY, in_g).unwrap();
    let body = f.done();
    assert_eq!(
        body.slot_vars(),
        [
            SlotVar { slot, var: pair, loc: at, expr: low },
            SlotVar { slot, var: pair, loc: at, expr: high },
            SlotVar { slot: other, var: whole, loc: at, expr: ExprId::EMPTY },
            SlotVar { slot, var: inlined, loc: in_g, expr: ExprId::EMPTY },
        ]
    );
    assert!(body.declares().is_empty(), "a slot's address needs no home");
    assert_eq!(body.dbg_record_count(), 0);
    // The instance of the inlined variable is its location's.
    let loc = f.m.debug.location(body.slot_vars()[3].loc).unwrap();
    assert_eq!((f.m.debug.subprogram_of(loc.scope), loc.inlined_at), (Some(f.g), call));
    assert_eq!(f.m.debug.function_of(in_g), Some(f.f));
}

/// §6.6: a declaration of an address that is not a slot is a record of
/// its own, and the value it names says so in its use facts without
/// being used.
#[test]
fn declarations_of_values_that_are_not_slots() {
    let mut f = fx();
    let vars: Vec<DiId> = (0..7).map(|n| f.var(format!("v{n}").as_bytes(), f.f)).collect();
    let deref = f.m.debug.expr(&[DiOp::Deref]).unwrap();
    let null = Value::constant(f.m.consts.null(&f.m.types, PTR).unwrap()).unwrap();
    let global = Value::sym(f.sym).unwrap();
    let (at, b) = (f.at, &mut f.b);
    let (number, pointer) = (b.arg(0).unwrap(), b.arg(1).unwrap());
    // An argument that is the variable's address (`sret`, `byval`).
    b.dbg_declare(pointer, vars[0], deref, at).unwrap();
    // An instruction's result: a dynamic `alloca`.
    let dynamic = b.alloca(PTR, number, 1, 4).unwrap();
    let loaded = b.load(PTR, dynamic, align(8)).unwrap();
    let next = b.load(I32, loaded, align(4)).unwrap();
    b.dbg_declare(dynamic, vars[1], ExprId::EMPTY, at).unwrap();
    b.dbg_declare(loaded, vars[2], ExprId::EMPTY, at).unwrap();
    // A constant, a symbol, and LLVM's empty metadata.
    b.dbg_declare(null, vars[3], ExprId::EMPTY, at).unwrap();
    b.dbg_declare(global, vars[4], ExprId::EMPTY, at).unwrap();
    b.dbg_declare(Value::NONE, vars[5], ExprId::EMPTY, at).unwrap();
    // A value that is defined later: an instruction, and a slot.
    let later = b.reserve(PTR).unwrap();
    let slot_later = b.reserve(PTR).unwrap();
    b.dbg_declare(later, vars[6], ExprId::EMPTY, at).unwrap();
    b.dbg_declare(slot_later, vars[6], deref, at).unwrap();
    let late = b.cast(Opcode::IntToPtr, PTR, next).unwrap();
    b.resolve(later, late).unwrap();
    let slot = b.add_slot(8, 8).unwrap();
    let slot_address = b.slot(slot).unwrap();
    b.resolve(slot_later, slot_address).unwrap();
    // What does not exist cannot be declared.
    let missing = Value::inst(InstId::new(99)).unwrap();
    assert_eq!(kind(b.dbg_declare(missing, vars[0], deref, at)), IrErrorKind::InvalidOperand);
    let asm = Value::asm(AsmId::new(0)).unwrap();
    assert_eq!(kind(b.dbg_declare(asm, vars[0], deref, at)), IrErrorKind::InvalidOperand);
    let body = f.done();

    let declare = |address, var, expr| DbgDeclare { address, var, expr, loc: at };
    assert_eq!(
        body.declares(),
        [
            declare(pointer, vars[0], deref),
            declare(dynamic, vars[1], ExprId::EMPTY),
            declare(loaded, vars[2], ExprId::EMPTY),
            declare(null, vars[3], ExprId::EMPTY),
            declare(global, vars[4], ExprId::EMPTY),
            declare(Value::NONE, vars[5], ExprId::EMPTY),
            declare(late, vars[6], ExprId::EMPTY),
        ]
    );
    // The reserved value that became a slot's address is a slot variable.
    assert_eq!(body.slot_vars(), [SlotVar { slot, var: vars[6], loc: at, expr: deref }]);
    // The fact, and that it is not a use.
    let id = |value: Value| value.as_inst().unwrap();
    assert!(body.arg_uses(1).named_by_declare() && body.arg_uses(1).is_unused());
    assert!(!body.arg_uses(0).named_by_declare());
    assert_eq!(body.arg_uses(0).count(), 2, "the alloca's count and the return");
    for value in [dynamic, loaded, late] {
        assert!(body.uses(id(value)).named_by_declare(), "{value:?}");
    }
    assert!(!body.uses(id(next)).named_by_declare());
    assert!(body.uses(id(dynamic)).only_use_is_next(), "the facts about uses are unchanged");
    assert!(body.uses(id(loaded)).only_use_is_next());
    assert!(body.uses(id(late)).is_unused());
    assert_eq!(format!("{:?}", body.uses(id(loaded))), "uses=1 next declared");
    assert_eq!(size_of::<DbgDeclare>(), 16);
    assert_eq!(size_of::<SlotVar>(), 16);
}

/// §6.6: value records stand before an instruction, in order, and name
/// values without using them; a record with an argument list names
/// several.
#[test]
fn value_records() {
    let mut f = fx();
    let (x, y) = (f.var(b"x", f.f), f.var(b"y", f.f));
    let sum = [DiOp::Arg(0), DiOp::Arg(1), DiOp::Plus, DiOp::StackValue];
    let sum = f.m.debug.expr(&sum).unwrap();
    let low = f.m.debug.expr(&[DiOp::Fragment(0, 16)]).unwrap();
    let undef = Value::constant(f.m.consts.undef(&f.m.types, I32).unwrap()).unwrap();
    let three = Value::constant(f.m.consts.int(&f.m.types, I32, 3).unwrap()).unwrap();
    let (at, b) = (f.at, &mut f.b);
    let (number, pointer) = (b.arg(0).unwrap(), b.arg(1).unwrap());
    b.dbg_value(number, x, ExprId::EMPTY, at).unwrap();
    let doubled = b.binary(Opcode::Add, I32, number, number).unwrap();
    let later = b.reserve(I32).unwrap();
    b.dbg_value(doubled, x, ExprId::EMPTY, at).unwrap();
    b.dbg_value_list(&[doubled, later], y, sum, at).unwrap();
    b.dbg_value(three, x, low, at).unwrap();
    let tripled = b.binary(Opcode::Add, I32, doubled, number).unwrap();
    b.resolve(later, tripled).unwrap();
    b.dbg_assign(tripled, y, ExprId::EMPTY, pointer, low, at).unwrap();
    b.dbg_value(undef, x, ExprId::EMPTY, at).unwrap();
    b.dbg_value(Value::NONE, y, ExprId::EMPTY, at).unwrap();
    b.dbg_value_list(&[], y, ExprId::EMPTY, at).unwrap();
    let many = vec![number; 65536];
    assert_eq!(kind(b.dbg_value_list(&many, y, sum, at)), IrErrorKind::InvalidOperand);
    let missing = Value::inst(InstId::new(99)).unwrap();
    assert_eq!(kind(b.dbg_value(missing, x, ExprId::EMPTY, at)), IrErrorKind::InvalidOperand);
    assert_eq!(kind(b.dbg_value_list(&[number, missing], x, sum, at)), IrErrorKind::InvalidOperand);
    assert_eq!(
        kind(b.dbg_assign(number, x, ExprId::EMPTY, missing, ExprId::EMPTY, at)),
        IrErrorKind::InvalidOperand
    );
    let ret = b.ret(Some(tripled)).unwrap();
    // A record stands before an instruction: it needs an open block.
    assert_eq!(kind(b.dbg_value(number, x, ExprId::EMPTY, at)), IrErrorKind::BadBlock);
    assert_eq!(kind(b.dbg_label(x, at)), IrErrorKind::BadBlock);
    let body = f.done();

    let id = |value: Value| value.as_inst().unwrap();
    let records: Vec<_> = body.dbg_records().collect();
    assert_eq!(records.len(), 8);
    assert_eq!(body.dbg_record_count(), 8);
    let read: Vec<_> = records
        .iter()
        .map(|record| (record.before, record.var, record.expr, record.values, record.arg_list))
        .collect();
    assert_eq!(
        read,
        [
            (id(doubled), x, ExprId::EMPTY, &[number][..], false),
            (id(tripled), x, ExprId::EMPTY, &[doubled][..], false),
            (id(tripled), y, sum, &[doubled, tripled][..], true),
            (id(tripled), x, low, &[three][..], false),
            (id(ret), y, ExprId::EMPTY, &[tripled][..], false),
            (id(ret), x, ExprId::EMPTY, &[undef][..], false),
            (id(ret), y, ExprId::EMPTY, &[Value::NONE][..], false),
            (id(ret), y, ExprId::EMPTY, &[][..], true),
        ]
    );
    for (index, record) in records.iter().enumerate() {
        let assign = index == 4;
        let kind = if assign { DbgRecordKind::Assign } else { DbgRecordKind::Value };
        assert_eq!((record.kind, record.loc), (kind, at), "record {index}");
        let address = if assign { (pointer, low) } else { (Value::NONE, ExprId::EMPTY) };
        assert_eq!((record.address, record.address_expr), address, "record {index}");
    }
    // Not uses: the counts are those of the instructions alone.
    assert_eq!(body.arg_uses(0).count(), 3);
    assert_eq!(body.arg_uses(1).count(), 0);
    assert_eq!(body.uses(id(doubled)).count(), 1);
    assert!(body.uses(id(doubled)).only_use_is_next());
    assert_eq!(body.uses(id(tripled)).count(), 1);
    assert!(!body.uses(id(tripled)).named_by_declare() && !body.arg_uses(1).named_by_declare());
    assert!(body.declares().is_empty() && body.slot_vars().is_empty());
}

/// §6.6: labels are records of their own kind, in the same position
/// order as the value records.
#[test]
fn labels() {
    let mut f = fx();
    let x = f.var(b"x", f.f);
    let label = |f: &mut Fx, name| {
        let node = DiLabel { scope: Some(f.f), name, file: Some(f.file), line: 5, ..Default::default() };
        f.m.debug.add(&node.into()).unwrap()
    };
    let (top, out) = (label(&mut f, b"top"), label(&mut f, b"out"));
    let (at, b) = (f.at, &mut f.b);
    let number = b.arg(0).unwrap();
    let next = b.create_block().unwrap();
    b.dbg_label(top, at).unwrap();
    b.dbg_value(number, x, ExprId::EMPTY, at).unwrap();
    let jump = b.br(next).unwrap();
    b.begin_block(next).unwrap();
    b.dbg_label(out, at).unwrap();
    let ret = b.ret(Some(number)).unwrap();
    let body = f.done();
    let read: Vec<_> = body
        .dbg_records()
        .map(|record| (record.kind, record.before, record.var, record.values.len()))
        .collect();
    let id = |value: Value| value.as_inst().unwrap();
    assert_eq!(
        read,
        [
            (DbgRecordKind::Label, id(jump), top, 0),
            (DbgRecordKind::Value, id(jump), x, 1),
            (DbgRecordKind::Label, id(ret), out, 0),
        ]
    );
    let first = body.dbg_records().next().unwrap();
    assert_eq!((first.loc, first.expr, first.address, first.arg_list), (at, ExprId::EMPTY, Value::NONE, false));
}

/// §6.5, §6.6: instructions inlined two deep carry locations whose
/// chain leads back to the function; so do the variables of the inlined
/// functions.
#[test]
fn inlined_at_chains_two_deep() {
    let mut f = fx();
    let block = DiLexicalBlock { scope: Some(f.h), file: Some(f.file), line: 31, column: 3 };
    let block = f.m.debug.add_distinct(&block.into()).unwrap();
    let (in_f, in_h) = (f.var(b"a", f.f), f.var(b"c", block));
    let call_g = f.m.debug.distinct_loc(DiLocation { line: 3, column: 9, scope: f.f, inlined_at: LocId::NONE }).unwrap();
    let call_h = f.loc(21, f.g, call_g);
    let deep = f.loc(32, block, call_h);
    let (at, b) = (f.at, &mut f.b);
    let number = b.arg(0).unwrap();
    b.set_loc(at);
    b.dbg_value(number, in_f, ExprId::EMPTY, at).unwrap();
    let first = b.binary(Opcode::Add, I32, number, number).unwrap();
    b.set_loc(call_h);
    let second = b.binary(Opcode::Mul, I32, first, first).unwrap();
    b.set_loc(deep);
    b.dbg_value(second, in_h, ExprId::EMPTY, deep).unwrap();
    let third = b.binary(Opcode::Sub, I32, second, first).unwrap();
    b.set_loc(LocId::NONE);
    b.ret(Some(third)).unwrap();
    let body = f.done();
    let d = &f.m.debug;
    let id = |value: Value| value.as_inst().unwrap();
    assert_eq!(body.loc(id(first)), at);
    assert_eq!(body.loc(id(second)), call_h);
    assert_eq!(body.loc(id(third)), deep);
    // From the innermost location outwards: h's block, g, f.
    let innermost = d.location(body.loc(id(third))).unwrap();
    assert_eq!((innermost.line, innermost.scope), (32, block));
    assert_eq!(d.subprogram_of(innermost.scope), Some(f.h));
    let middle = d.location(innermost.inlined_at).unwrap();
    assert_eq!((middle.line, middle.scope, middle.inlined_at), (21, f.g, call_g));
    let outer = d.location(middle.inlined_at).unwrap();
    assert_eq!((outer.line, outer.scope, outer.inlined_at), (3, f.f, LocId::NONE));
    for value in [first, second, third] {
        assert_eq!(d.function_of(body.loc(id(value))), Some(f.f));
    }
    let records: Vec<_> = body.dbg_records().collect();
    assert_eq!(d.location(records[1].loc).unwrap().inlined_at, call_h);
}

/// §6.8: the rules `Body::verify` holds a body's debug information to
/// when its module has debug tables, each with a body that keeps it and
/// one that breaks it.
#[test]
fn verify_accepts_and_rejects() {
    let invalid = Some(IrErrorKind::InvalidDebugInfo);
    let check = |build: &dyn Fn(&mut Fx)| {
        let mut f = fx();
        build(&mut f);
        f.verdict()
    };
    assert_eq!(check(&|_| {}), None);

    // An instruction's location is a location of the module ...
    assert_eq!(check(&|f| f.b.set_loc(f.at)), None);
    assert_eq!(check(&|f| f.b.set_loc(LocId::new(500))), invalid);
    // ... in the function's subprogram or inlined into it.
    assert_eq!(
        check(&|f| {
            let call = f.loc(2, f.f, LocId::NONE);
            let inlined = f.loc(9, f.g, call);
            f.b.set_loc(inlined);
        }),
        None
    );
    assert_eq!(
        check(&|f| {
            let elsewhere = f.loc(9, f.g, LocId::NONE);
            f.b.set_loc(elsewhere);
        }),
        invalid
    );
    assert_eq!(
        check(&|f| {
            let call = f.loc(2, f.g, LocId::NONE);
            let inlined = f.loc(9, f.f, call);
            f.b.set_loc(inlined);
        }),
        invalid,
        "the chain ends in g"
    );

    // A variable is a local variable; a label is a label.
    let slot_var = |f: &mut Fx, var, expr, loc| {
        let slot = f.b.add_slot(4, 4).unwrap();
        let address = f.b.slot(slot).unwrap();
        f.b.dbg_declare(address, var, expr, loc).unwrap();
    };
    assert_eq!(
        check(&|f| {
            let var = f.var(b"x", f.f);
            slot_var(f, var, ExprId::EMPTY, f.at);
        }),
        None
    );
    assert_eq!(check(&|f| slot_var(f, f.int, ExprId::EMPTY, f.at)), invalid);
    assert_eq!(check(&|f| slot_var(f, DiId::new(900), ExprId::EMPTY, f.at)), invalid);
    let label = |f: &mut Fx, scope| {
        let node = DiLabel { scope: Some(scope), name: b"l", ..Default::default() };
        f.m.debug.add(&node.into()).unwrap()
    };
    assert_eq!(
        check(&|f| {
            let node = label(f, f.f);
            f.b.dbg_label(node, f.at).unwrap();
        }),
        None
    );
    assert_eq!(
        check(&|f| {
            let var = f.var(b"x", f.f);
            f.b.dbg_label(var, f.at).unwrap();
        }),
        invalid
    );
    assert_eq!(
        check(&|f| {
            let node = label(f, f.f);
            let value = f.b.arg(0).unwrap();
            f.b.dbg_value(value, node, ExprId::EMPTY, f.at).unwrap();
        }),
        invalid
    );

    // A declaration or record has a location, in the subprogram of its
    // variable or label.
    let declare = |f: &mut Fx, scope, expr, loc| {
        let var = f.var(b"x", scope);
        let address = f.b.arg(1).unwrap();
        f.b.dbg_declare(address, var, expr, loc).unwrap();
    };
    assert_eq!(check(&|f| declare(f, f.f, ExprId::EMPTY, f.at)), None);
    assert_eq!(check(&|f| declare(f, f.f, ExprId::EMPTY, LocId::NONE)), invalid);
    assert_eq!(check(&|f| declare(f, f.g, ExprId::EMPTY, f.at)), invalid);
    assert_eq!(
        check(&|f| {
            let call = f.loc(2, f.f, LocId::NONE);
            let inlined = f.loc(9, f.g, call);
            declare(f, f.g, ExprId::EMPTY, inlined);
        }),
        None
    );
    assert_eq!(
        check(&|f| {
            let block = DiLexicalBlock { scope: Some(f.f), line: 2, ..Default::default() };
            let block = f.m.debug.add_distinct(&block.into()).unwrap();
            declare(f, block, ExprId::EMPTY, f.at);
        }),
        None,
        "a block of the subprogram is in it"
    );
    assert_eq!(
        check(&|f| {
            let node = label(f, f.g);
            f.b.dbg_label(node, f.at).unwrap();
        }),
        invalid
    );
    assert_eq!(
        check(&|f| {
            let var = f.var(b"x", f.f);
            let value = f.b.arg(0).unwrap();
            let elsewhere = f.loc(9, f.g, LocId::NONE);
            f.b.dbg_value(value, var, ExprId::EMPTY, elsewhere).unwrap();
        }),
        invalid,
        "the record is in another function"
    );

    // An expression exists and names only the values its record has.
    assert_eq!(check(&|f| declare(f, f.f, ExprId::new(40), f.at)), invalid);
    let arg = |f: &mut Fx, index| f.m.debug.expr(&[DiOp::Arg(index), DiOp::Deref]).unwrap();
    assert_eq!(
        check(&|f| {
            let expr = arg(f, 0);
            declare(f, f.f, expr, f.at);
        }),
        None
    );
    assert_eq!(
        check(&|f| {
            let expr = arg(f, 1);
            declare(f, f.f, expr, f.at);
        }),
        invalid
    );
    let list = |f: &mut Fx, count: usize, index| {
        let var = f.var(b"x", f.f);
        let expr = arg(f, index);
        let values = vec![f.b.arg(0).unwrap(); count];
        f.b.dbg_value_list(&values, var, expr, f.at).unwrap();
    };
    assert_eq!(check(&|f| list(f, 3, 2)), None);
    assert_eq!(check(&|f| list(f, 3, 3)), invalid);
    assert_eq!(check(&|f| list(f, 0, 0)), invalid);
    assert_eq!(
        check(&|f| {
            let var = f.var(b"x", f.f);
            let (value, address) = (f.b.arg(0).unwrap(), f.b.arg(1).unwrap());
            f.b.dbg_assign(value, var, ExprId::EMPTY, address, ExprId::new(40), f.at).unwrap();
        }),
        invalid
    );

    // A record does not stand before a phi.
    let before_phi = |f: &mut Fx, early: bool| {
        let var = f.var(b"x", f.f);
        let entry = f.b.current_block().unwrap();
        let value = f.b.arg(0).unwrap();
        let next = f.b.create_block().unwrap();
        f.b.br(next).unwrap();
        f.b.begin_block(next).unwrap();
        if early {
            f.b.dbg_value(value, var, ExprId::EMPTY, f.at).unwrap();
        }
        let phi = f.b.phi(I32, &[(value, entry)]).unwrap();
        f.b.dbg_value(phi, var, ExprId::EMPTY, f.at).unwrap();
    };
    assert_eq!(check(&|f| before_phi(f, false)), None);
    assert_eq!(check(&|f| before_phi(f, true)), invalid);
}

/// §6.8: a function without a subprogram has no debug information to
/// place its locations in, and a module without debug tables has none
/// to check ids against: `verify` checks what it can.
#[test]
fn verify_without_a_subprogram_or_without_tables() {
    // No subprogram: locations must exist, and may be in any function.
    let mut f = fx();
    let plain = f.m.symbol_named(b"plain").unwrap();
    f.m.set_function(plain, &FunctionDesc { has_body: true, ..FunctionDesc::new(f.fn_ty) }).unwrap();
    let in_g = f.loc(9, f.g, LocId::NONE);
    for (loc, verdict) in [(in_g, None), (LocId::new(77), Some(IrErrorKind::InvalidDebugInfo))] {
        let mut b = FunctionBuilder::new(&f.m, plain, f.fn_ty).unwrap();
        let entry = b.create_block().unwrap();
        b.begin_block(entry).unwrap();
        b.set_loc(loc);
        let value = b.arg(0).unwrap();
        b.ret(Some(value)).unwrap();
        let body = b.finish().unwrap();
        assert_eq!(body.verify(&f.m).err().map(|error| error.kind), verdict);
    }

    // No tables: ids are not checked, positions and values are.
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let fn_ty = m.types.function(I32, &[I32], false).unwrap();
    let mut b = FunctionBuilder::new(&m, SymId::new(0), fn_ty).unwrap();
    let entry = b.create_block().unwrap();
    b.begin_block(entry).unwrap();
    let value = b.arg(0).unwrap();
    b.set_loc(LocId::new(4));
    b.dbg_value(value, DiId::new(8), ExprId::new(3), LocId::new(5)).unwrap();
    b.dbg_label(DiId::new(9), LocId::NONE).unwrap();
    b.dbg_declare(value, DiId::new(8), ExprId::EMPTY, LocId::new(5)).unwrap();
    b.ret(Some(value)).unwrap();
    let body = b.finish().unwrap();
    assert!(m.debug.is_empty());
    body.verify(&m).unwrap();
    assert!(body.arg_uses(0).named_by_declare());
}

/// §6.1, §6.10: a body without debug information holds nothing for it:
/// two empty tables. With it, a declaration costs 16 bytes and a record
/// 28 and four for each value.
#[test]
fn a_body_without_debug_information_pays_two_empty_tables() {
    let build = |debug: bool| {
        let mut f = fx();
        let x = f.var(b"x", f.f);
        let (at, b) = (f.at, &mut f.b);
        let (number, pointer) = (b.arg(0).unwrap(), b.arg(1).unwrap());
        let slot = b.add_slot(4, 4).unwrap();
        let address = b.slot(slot).unwrap();
        if debug {
            b.dbg_declare(address, x, ExprId::EMPTY, at).unwrap();
            b.dbg_declare(pointer, x, ExprId::EMPTY, at).unwrap();
            b.dbg_value(number, x, ExprId::EMPTY, at).unwrap();
            b.dbg_value_list(&[number, number, number], x, ExprId::EMPTY, at).unwrap();
            b.dbg_assign(number, x, ExprId::EMPTY, pointer, ExprId::EMPTY, at).unwrap();
            b.dbg_label(x, at).unwrap();
        }
        b.store(number, address, align(4)).unwrap();
        f.finish()
    };
    let (plain, with) = (build(false), build(true));
    assert!(plain.slot_vars().is_empty() && plain.declares().is_empty());
    assert_eq!(plain.dbg_record_count(), 0);
    assert_eq!(plain.dbg_records().count(), 0);
    assert!(!plain.arg_uses(1).named_by_declare());
    // One slot variable, one declaration, four records, 1 + 3 + (1 + 1)
    // values.
    assert_eq!(with.table_bytes() - plain.table_bytes(), 16 + 16 + 4 * 28 + 6 * 4);
    assert_eq!(plain.inst_count(), with.inst_count());
    for index in 0..plain.inst_count() as u32 {
        let id = InstId::new(index);
        assert_eq!(plain.uses(id), with.uses(id), "records are not uses");
    }
    assert_eq!(plain.arg_uses(0), with.arg_uses(0));
    // Two more empty tables than before debug records existed: 48 bytes
    // in a struct there is one of per function being compiled.
    assert_eq!(size_of::<Body>(), 544);
}
