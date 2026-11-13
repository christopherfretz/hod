//! Metadata operands (WP-064, `spec/ir.md` §5.3, §5.5, §5.8, §8, §9.4):
//! built, read back, verified, classified and printed; refused wherever
//! LLVM does not allow metadata.

use hod_ir::{
    CallDesc, Dialect, FunctionBuilder, FunctionDesc, Intrinsic, IrErrorKind, Module, Opcode,
    SymId, TargetSpec, TypeId, Value, ValueKind, classify_body,
};

struct Fixture {
    m: Module,
    read_ty: TypeId,
    write_ty: TypeId,
    sym: SymId,
}

fn fixture() -> Fixture {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let read_ty = m.types.function(TypeId::I64, &[TypeId::METADATA], false).unwrap();
    let write_ty = m.types.function(TypeId::VOID, &[TypeId::METADATA, TypeId::I64], false).unwrap();
    let fn_ty = m.types.function(TypeId::I64, &[TypeId::I64], false).unwrap();
    m.add_function(b"llvm.read_register.i64", &FunctionDesc::new(read_ty)).unwrap();
    m.add_function(b"llvm.write_register.i64", &FunctionDesc::new(write_ty)).unwrap();
    let sym =
        m.add_function(b"f", &FunctionDesc { has_body: true, ..FunctionDesc::new(fn_ty) }).unwrap();
    Fixture { m, read_ty, write_ty, sym }
}

fn builder(f: &Fixture) -> FunctionBuilder {
    let fn_ty = f.m.symbol(f.sym).ty();
    let mut b = FunctionBuilder::new(&f.m, f.sym, fn_ty).unwrap();
    let entry = b.create_block().unwrap();
    b.begin_block(entry).unwrap();
    b
}

// §5.3: a node of strings and a string, read back as built; the value
// is four bytes like any other and has type `metadata`.
#[test]
fn metadata_operands_read_back_and_verify() {
    let f = fixture();
    let mut b = builder(&f);
    let sp = b.metadata(true, &["sp"]).unwrap();
    let pair = b.metadata(true, &["a", "b\"c"]).unwrap();
    let empty = b.metadata(true, &[]).unwrap();
    let string = b.metadata(false, &["fpcr"]).unwrap();
    assert_eq!(b.metadata(false, &[]).unwrap_err().kind, IrErrorKind::InvalidOperand);
    assert_eq!(b.metadata(false, &["a", "b"]).unwrap_err().kind, IrErrorKind::InvalidOperand);
    let overloads = [TypeId::I64];
    let read = |meta| [meta];
    let args = read(sp);
    let a = b
        .call(
            TypeId::I64,
            &CallDesc::intrinsic(Intrinsic::ReadRegister, &overloads, f.read_ty, &args),
        )
        .unwrap();
    let wargs = [string, a];
    b.call(
        TypeId::VOID,
        &CallDesc::intrinsic(Intrinsic::WriteRegister, &overloads, f.write_ty, &wargs),
    )
    .unwrap();
    b.ret(Some(a)).unwrap();
    let body = b.finish().unwrap();
    body.verify(&f.m).unwrap();
    assert_eq!(body.meta_count(), 4);
    let text = |value: Value| {
        let meta = body.metadata(value.as_meta().unwrap()).unwrap();
        (meta.is_node(), meta.strings().collect::<Vec<_>>())
    };
    assert_eq!(text(sp), (true, vec!["sp"]));
    assert_eq!(text(pair), (true, vec!["a", "b\"c"]));
    assert_eq!(text(empty), (true, vec![]));
    assert_eq!(text(string), (false, vec!["fpcr"]));
    let sp_meta = body.metadata(sp.as_meta().unwrap()).unwrap();
    assert_eq!(sp_meta.single_node_string(), Some("sp"));
    assert_eq!(body.metadata(pair.as_meta().unwrap()).unwrap().single_node_string(), None);
    assert_eq!(body.metadata(string.as_meta().unwrap()).unwrap().single_node_string(), None);
    assert!(matches!(sp.kind(), ValueKind::Meta(_)));
    assert_eq!(body.value_type(&f.m, sp), TypeId::METADATA);
    assert_eq!(format!("{sp_meta:?}"), "[\"sp\"]");
    // §8: classify knows the intrinsics; what the register is, is the
    // target's to judge.
    classify_body(&f.m, &body).unwrap();
}

// §5.3: metadata is an argument of an intrinsic call only.
#[test]
fn metadata_anywhere_else_is_refused() {
    let f = fixture();
    let mut b = builder(&f);
    let sp = b.metadata(true, &["sp"]).unwrap();
    let kind = |r: hod_ir::IrResult<Value>| r.unwrap_err().kind;
    assert_eq!(kind(b.binary(Opcode::Add, TypeId::I64, sp, sp)), IrErrorKind::InvalidOperand);
    assert_eq!(kind(b.ret(Some(sp))), IrErrorKind::InvalidOperand);
    // A call of a function value is no intrinsic call.
    let callee = Value::sym(f.sym).unwrap();
    let args = [sp];
    assert_eq!(
        kind(b.call(TypeId::I64, &CallDesc::new(callee, f.read_ty, &args))),
        IrErrorKind::InvalidOperand
    );
    // A metadata operand of another body does not exist in this one.
    let mut other = builder(&f);
    let foreign = Value::meta(hod_ir::MetaId::new(5)).unwrap();
    let foreign_args = [foreign];
    let overloads = [TypeId::I64];
    assert_eq!(
        kind(other.call(
            TypeId::I64,
            &CallDesc::intrinsic(Intrinsic::ReadRegister, &overloads, f.read_ty, &foreign_args)
        )),
        IrErrorKind::InvalidOperand
    );
}

// §5.8: the verifier refuses a metadata operand in a parameter that is not
// `metadata`, and a `metadata` parameter given a value.
#[test]
fn the_verifier_checks_the_parameter_type() {
    let mut f = fixture();
    // An intrinsic whose first parameter is an integer, called with
    // metadata in it: the builder lets it through, the verifier does not.
    let wrong_ty = f.m.types.function(TypeId::I64, &[TypeId::I64], false).unwrap();
    let overloads = [TypeId::I64];
    let mut b = builder(&f);
    let sp = b.metadata(true, &["sp"]).unwrap();
    let args = [sp];
    let a = b
        .call(
            TypeId::I64,
            &CallDesc::intrinsic(Intrinsic::Other("llvm.x.i64"), &[], wrong_ty, &args),
        )
        .unwrap();
    b.ret(Some(a)).unwrap();
    let body = b.finish().unwrap();
    assert_eq!(body.verify(&f.m).unwrap_err().kind, IrErrorKind::InvalidOperand);

    // A `metadata` parameter given an integer.
    let mut b = builder(&f);
    let int = b.arg(0).unwrap();
    let args = [int];
    let a = b
        .call(
            TypeId::I64,
            &CallDesc::intrinsic(Intrinsic::ReadRegister, &overloads, f.read_ty, &args),
        )
        .unwrap();
    b.ret(Some(a)).unwrap();
    let body = b.finish().unwrap();
    assert!(body.verify(&f.m).is_err());
    f.read_ty = wrong_ty;
}

// §9.4: the printer writes the operand in place, and LLVM's tools read
// what it writes (the round trip itself is `cargo xtask roundtrip`).
#[test]
fn the_printer_writes_metadata_in_place() {
    let f = fixture();
    let mut b = builder(&f);
    let sp = b.metadata(true, &["sp", "a\\b"]).unwrap();
    let string = b.metadata(false, &["fpcr"]).unwrap();
    let overloads = [TypeId::I64];
    let args = [sp];
    let a = b
        .call(
            TypeId::I64,
            &CallDesc::intrinsic(Intrinsic::ReadRegister, &overloads, f.read_ty, &args),
        )
        .unwrap();
    let args = [string];
    b.call(
        TypeId::I64,
        &CallDesc::intrinsic(Intrinsic::ReadRegister, &overloads, f.read_ty, &args),
    )
    .unwrap();
    b.ret(Some(a)).unwrap();
    let body = b.finish().unwrap();
    body.verify(&f.m).unwrap();
    for dialect in [Dialect::LLVM19, Dialect::LLVM22] {
        let mut text = String::new();
        f.m.print(&mut text, dialect, [&body]).unwrap();
        assert!(
            text.contains("call i64 @llvm.read_register.i64(metadata !{!\"sp\", !\"a\\5Cb\"})"),
            "{text}"
        );
        assert!(text.contains("call i64 @llvm.read_register.i64(metadata !\"fpcr\")"), "{text}");
        let major = dialect.major();
        let Some(as_tool) = hod_testutil::find_tool(&format!("llvm-as-{major}")).or_else(|| {
            let path = std::path::PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llvm-as"));
            path.is_file().then_some(path)
        }) else {
            continue;
        };
        let mut command = std::process::Command::new(as_tool);
        command.args(["-o", "/dev/null", "-"]);
        let outcome = hod_testutil::run_with(
            &mut command,
            text.as_bytes(),
            std::time::Duration::from_mins(1),
        );
        assert!(outcome.success(), "llvm-as {major}: {}\n{text}", outcome.stderr_text());
    }
}
