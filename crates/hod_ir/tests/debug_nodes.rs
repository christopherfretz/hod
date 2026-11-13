//! L0 tests of the debug tables (`spec/ir.md` §6.1 to §6.5, §6.7, §6.8,
//! §6.10): every kind of node and every field is built and read back,
//! graphs with cycles are built by reserving, every builder error is
//! provoked, the uniquing rules are held to, and `Module::verify_debug`
//! accepts the well-formed tables and names the rule each ill-formed one
//! breaks.

use hod_ir::{
    DI_FIELD_NOTES, DI_KIND_NOTES, DI_OPS, DI_RECORDS, DW_TAG_CLASS_TYPE, DW_TAG_MEMBER,
    DW_TAG_POINTER_TYPE, DW_TAG_STRUCTURE_TYPE, DebugInfo, DebugStats, DiBasicType, DiBound,
    DiCompileUnit, DiCompositeType, DiConstant, DiDerivedType, DiEnumerator, DiFieldValue, DiFile,
    DiFlags, DiGlobalVariable, DiGlobalVariableExpression, DiId, DiKind, DiLabel, DiLexicalBlock,
    DiLexicalBlockFile, DiLocalVariable, DiLocation, DiMapping, DiNamespace, DiNode, DiOp, DiOther,
    DiString, DiSubprogram, DiSubrange, DiSubroutineType, DiTemplateValueParameter, DiTuple,
    EmissionKind, ExprId, FlagValue, FunctionDesc, IrErrorKind, IrResult, LocId, Module,
    NameTableKind, SpFlags, SymId, TargetSpec, TypeId, VariableDesc,
};

fn kind<T: std::fmt::Debug>(result: IrResult<T>) -> IrErrorKind {
    result.unwrap_err().kind
}

fn module() -> Module {
    Module::new(TargetSpec::x86_64_linux_gnu())
}

/// A compile unit, its file, a subprogram definition `f` and the `int`
/// type: what most tests start from.
#[allow(clippy::struct_field_names, reason = "`unit` is the compile unit")]
struct Unit {
    m: Module,
    file: DiId,
    unit: DiId,
    int: DiId,
    f: DiId,
}

fn unit() -> Unit {
    let mut m = module();
    let d = &mut m.debug;
    let file = d
        .add(&DiFile { filename: b"a.c", directory: b"/src", ..Default::default() }.into())
        .unwrap();
    let unit = d
        .add_distinct(
            &DiCompileUnit {
                language: 0x1d,
                file: Some(file),
                producer: b"test",
                emission_kind: EmissionKind::FullDebug,
                ..Default::default()
            }
            .into(),
        )
        .unwrap();
    d.add_compile_unit(unit).unwrap();
    let int = d
        .add(&DiBasicType { name: b"int", size: 32, encoding: 5, ..Default::default() }.into())
        .unwrap();
    let ty = d.add(&DiSubroutineType { types: &[int, int], ..Default::default() }.into()).unwrap();
    let f = d
        .add_distinct(
            &DiSubprogram {
                name: b"f",
                scope: Some(file),
                file: Some(file),
                line: 3,
                ty: Some(ty),
                scope_line: 3,
                sp_flags: SpFlags::DEFINITION,
                unit: Some(unit),
                ..Default::default()
            }
            .into(),
        )
        .unwrap();
    Unit { m, file, unit, int, f }
}

/// The values a field of some shape can be given, a default one excluded.
fn candidates(node: DiId, list: &[DiId], expr: ExprId) -> [(&'static str, DiFieldValue<'_>); 9] {
    [
        ("string", DiFieldValue::Str(b"some text")),
        ("number", DiFieldValue::UInt(1)),
        ("number", DiFieldValue::UInt(0x4107)),
        ("number", DiFieldValue::UInt(u64::from(u32::MAX))),
        ("wide number", DiFieldValue::UInt(0xfedc_ba98_7654_3210)),
        ("signed", DiFieldValue::Int(-5)),
        ("true", DiFieldValue::Bool(true)),
        ("node", DiFieldValue::Node(Some(node))),
        ("list", DiFieldValue::List(list)),
    ]
    .map(|(name, value)| match (name, value) {
        // The last candidate doubles as the expression.
        ("list", _) if list.is_empty() => ("expression", DiFieldValue::Expr(expr)),
        other => other,
    })
}

/// §6.2: every field of every kind, given a value of each shape it
/// takes, reads back as it was written: alone (every other field at its
/// default, so absent from the stored node) and with every field set.
#[test]
fn every_field_of_every_kind_round_trips() {
    let mut d = DebugInfo::default();
    let target = d.add(&DiFile { filename: b"t", ..Default::default() }.into()).unwrap();
    let expr = d.expr(&[DiOp::Deref]).unwrap();
    let list = [target, DiId::NONE, target];
    let mut fields = 0;
    for &kind in DiKind::ALL {
        // A constant is not made from LLVM's field values.
        if kind == DiKind::Constant {
            continue;
        }
        let mut full = DiNode::new(kind);
        for field in kind.fields() {
            let mut shapes = 0;
            for values in [candidates(target, &list, expr), candidates(target, &[], expr)] {
                for (shape, value) in values {
                    let mut node = DiNode::new(kind);
                    if node.set(field.as_bytes(), value).is_err() {
                        continue;
                    }
                    assert_ne!(node, DiNode::new(kind), "{kind:?}.{field} = {shape}");
                    let id = d.add_distinct(&node).unwrap();
                    assert_eq!(d.node(id), node, "{kind:?}.{field} = {shape}");
                    assert_eq!(d.kind(id), kind);
                    assert!(d.is_distinct(id));
                    full.set(field.as_bytes(), value).unwrap();
                    shapes += 1;
                }
            }
            assert!(shapes > 0, "{kind:?}.{field} takes no value");
            fields += 1;
        }
        let id = d.add_distinct(&full).unwrap();
        assert_eq!(d.node(id), full, "{kind:?} with every field set");
        // The default node: a kind, a tag and an empty mask.
        let before = d.stats().node_bytes;
        let id = d.add_distinct(&DiNode::new(kind)).unwrap();
        assert_eq!(d.node(id), DiNode::new(kind));
        assert_eq!(d.stats().node_bytes - before, 12, "{kind:?}");
    }
    // Every mapped row of the field table was a field above: the rows
    // are these fields, less the three kinds that are not `!DI...` nodes
    // (one field each), plus the ten mapped rows that are not fields
    // of a record (`DILocation`'s six, with WP-149's `atomGroup` and
    // `atomRank`; a subprogram's four older ones).
    let mapped = hod_ir::di_field_table().filter(|row| row.2 == DiMapping::Mapped).count();
    assert_eq!(fields - 3, mapped - 10);
    assert!(fields > 130, "{fields}");
}

/// §6.2: the typed records, as a producer writes them; the tag is a
/// field of the kinds that have one, and the three kinds for metadata
/// that is not a debug node.
#[test]
fn typed_records_round_trip() {
    let Unit { mut m, file, unit, int, f } = unit();
    let nine = m.consts.int(&m.types, TypeId::I32, 9).unwrap();
    let d = &mut m.debug;
    assert_eq!(d.kind(unit), DiKind::CompileUnit);
    let DiNode::CompileUnit(read) = d.node(unit) else { panic!() };
    assert_eq!((read.language, read.file, read.producer), (0x1d, Some(file), &b"test"[..]));
    assert_eq!(read.emission_kind, EmissionKind::FullDebug);
    assert_eq!(read.name_table_kind, NameTableKind::Default);
    assert!(read.enums.is_empty() && !read.is_optimized);
    assert_eq!(d.tag(unit), 0);

    let pointer = DiDerivedType {
        tag: DW_TAG_POINTER_TYPE,
        base_type: Some(int),
        size: 64,
        dwarf_address_space: Some(0),
        ..Default::default()
    };
    let id = d.add(&pointer.into()).unwrap();
    assert_eq!(d.tag(id), DW_TAG_POINTER_TYPE);
    assert_eq!(d.node(id), DiNode::DerivedType(pointer));
    let plain = DiDerivedType { dwarf_address_space: None, ..pointer };
    assert_ne!(d.add(&plain.into()).unwrap(), id, "address space 0 is not no address space");

    let ranges = [
        DiSubrange { count: DiBound::Const(-1), ..Default::default() },
        DiSubrange {
            lower_bound: DiBound::Const(0),
            upper_bound: DiBound::Node(int),
            stride: DiBound::Expr(ExprId::EMPTY),
            ..Default::default()
        },
    ];
    for range in ranges {
        let id = d.add(&range.into()).unwrap();
        assert_eq!(d.node(id), DiNode::Subrange(range));
    }
    for value in [i64::MIN, -1, 0, i64::MAX] {
        let node = DiEnumerator { name: b"e", value, is_unsigned: value == -1 };
        let id = d.add(&node.into()).unwrap();
        assert_eq!(d.node(id), DiNode::Enumerator(node));
    }

    let constant = d.add(&DiConstant { value: nine }.into()).unwrap();
    assert_eq!(d.node(constant), DiNode::Constant(DiConstant { value: nine }));
    let string = d.add(&DiString { text: b"std::vector" }.into()).unwrap();
    let tuple = d.add(&DiTuple { elements: &[constant, DiId::NONE, string] }.into()).unwrap();
    let DiNode::Tuple(read) = d.node(tuple) else { panic!() };
    assert_eq!(read.elements, [constant, DiId::NONE, string]);
    let param = DiTemplateValueParameter {
        name: b"N",
        ty: Some(int),
        value: Some(constant),
        ..Default::default()
    };
    let id = d.add(&param.into()).unwrap();
    assert_eq!(d.node(id), DiNode::TemplateValueParameter(param));
    assert_eq!(d.tag(id), hod_ir::DW_TAG_TEMPLATE_VALUE_PARAMETER);

    // Strings are bytes, and the empty string is the absent one.
    let odd = DiNamespace { name: b"\xff\0n", scope: Some(file), export_symbols: true };
    let id = d.add(&odd.into()).unwrap();
    assert_eq!(d.node(id), DiNode::Namespace(odd));
    let anonymous = d.add(&DiNamespace::default().into()).unwrap();
    assert_eq!(
        d.node(anonymous),
        DiNode::Namespace(DiNamespace { name: b"", ..Default::default() })
    );

    let DiNode::Subprogram(read) = d.node(f) else { panic!() };
    assert_eq!((read.name, read.line, read.unit), (&b"f"[..], 3, Some(unit)));
    assert!(read.sp_flags.contains(SpFlags::DEFINITION) && read.linkage_name.is_empty());
    assert!(
        d.contains(f) && !d.contains(DiId::new(d.node_count() as u32)) && !d.contains(DiId::NONE)
    );
    assert_eq!(d.find_kind(DiId::NONE), None);
    m.finish_debug().unwrap();
    m.verify_debug().unwrap();
    assert_eq!(m.unsupported().len(), 0);
}

/// §6.3: a graph with cycles is built by reserving: a class names its
/// members and each member names the class; a definition names its
/// declaration, which is a member of the class the definition is scoped
/// in.
#[test]
fn cycles_are_built_by_reserving() {
    let Unit { mut m, file, unit, int, .. } = unit();
    let d = &mut m.debug;
    let class = d.reserve().unwrap();
    assert_eq!(d.kind(class), DiKind::Undefined);
    assert_eq!(d.node(class), DiNode::Undefined);
    let member = d
        .add(
            &DiDerivedType {
                tag: DW_TAG_MEMBER,
                name: b"x",
                scope: Some(class),
                file: Some(file),
                base_type: Some(int),
                size: 32,
                ..Default::default()
            }
            .into(),
        )
        .unwrap();
    let this = d
        .add(
            &DiDerivedType {
                tag: DW_TAG_POINTER_TYPE,
                base_type: Some(class),
                size: 64,
                ..Default::default()
            }
            .into(),
        )
        .unwrap();
    let method_ty = d
        .add(&DiSubroutineType { types: &[DiId::NONE, this], ..Default::default() }.into())
        .unwrap();
    let declaration = d
        .add(
            &DiSubprogram {
                name: b"get",
                scope: Some(class),
                ty: Some(method_ty),
                ..Default::default()
            }
            .into(),
        )
        .unwrap();
    // The definition is met before the node it will be: reserved first.
    let definition = d.reserve().unwrap();
    d.define_distinct(
        definition,
        &DiSubprogram {
            name: b"get",
            scope: Some(class),
            ty: Some(method_ty),
            sp_flags: SpFlags::DEFINITION,
            unit: Some(unit),
            declaration: Some(declaration),
            ..Default::default()
        }
        .into(),
    )
    .unwrap();
    assert_eq!(kind(m.finish_debug()), IrErrorKind::UndefinedDebugNode, "the class is not defined");
    let d = &mut m.debug;
    d.define_distinct(
        class,
        &DiCompositeType {
            tag: DW_TAG_CLASS_TYPE,
            name: b"C",
            file: Some(file),
            size: 32,
            elements: &[member, declaration],
            identifier: b"_ZTS1C",
            ..Default::default()
        }
        .into(),
    )
    .unwrap();
    let DiNode::CompositeType(read) = d.node(class) else { panic!() };
    assert_eq!(read.elements, [member, declaration]);
    let DiNode::DerivedType(read) = d.node(member) else { panic!() };
    assert_eq!(read.scope, Some(class));
    assert!(d.is_distinct(class) && d.is_distinct(definition) && !d.is_distinct(member));
    m.finish_debug().unwrap();
    m.verify_debug().unwrap();
}

/// §6.3: the errors of reserving and defining, each of which leaves the
/// table as it was.
#[test]
fn reserve_and_define_errors() {
    let Unit { mut m, file, int, .. } = unit();
    let d = &mut m.debug;
    let nodes = d.node_count();
    let stats = d.stats();
    let missing = DiId::new(nodes as u32);
    let file_node: DiNode<'_> = DiFile { filename: b"b.c", ..Default::default() }.into();

    // A field that names a node that was never reserved or added.
    let types = [int, missing];
    let dangling = [
        DiNode::from(DiLexicalBlock { scope: Some(missing), ..Default::default() }),
        DiSubroutineType { types: &types, ..Default::default() }.into(),
        DiSubrange { count: DiBound::Node(missing), ..Default::default() }.into(),
        DiSubrange { count: DiBound::Expr(ExprId::new(1)), ..Default::default() }.into(),
        DiGlobalVariableExpression { var: None, expr: ExprId::new(1) }.into(),
    ];
    for node in &dangling {
        assert_eq!(kind(d.add(node)), IrErrorKind::UndefinedDebugNode, "{node:?}");
        assert_eq!(kind(d.add_distinct(node)), IrErrorKind::UndefinedDebugNode, "{node:?}");
    }
    // A node that is nothing, and nodes LLVM requires to be distinct.
    assert_eq!(kind(d.add(&DiNode::Undefined)), IrErrorKind::InvalidDebugInfo);
    assert_eq!(kind(d.add(&DiCompileUnit::default().into())), IrErrorKind::InvalidDebugInfo);
    let definition = DiSubprogram { sp_flags: SpFlags::DEFINITION, ..Default::default() };
    assert_eq!(kind(d.add(&definition.into())), IrErrorKind::InvalidDebugInfo);
    // Defining what was not reserved.
    assert_eq!(kind(d.define(missing, &file_node)), IrErrorKind::UndefinedDebugNode);
    assert_eq!(kind(d.define_distinct(DiId::NONE, &file_node)), IrErrorKind::UndefinedDebugNode);
    assert_eq!(kind(d.define(file, &file_node)), IrErrorKind::DebugRedefined, "added, so defined");
    assert_eq!((d.node_count(), d.stats()), (nodes, stats), "nothing was left behind");
    d.check_defined().unwrap();

    let reserved = d.reserve().unwrap();
    assert_eq!(reserved, missing);
    let error = d.check_defined().unwrap_err();
    assert_eq!(error.kind, IrErrorKind::UndefinedDebugNode);
    assert!(error.message.contains(&format!("{reserved:?}")), "{error}");
    assert_eq!(kind(d.verify()), IrErrorKind::UndefinedDebugNode);
    for node in &dangling[..1] {
        // The reserved id itself can be named now; a later one cannot.
        d.add(node).unwrap();
    }
    assert_eq!(kind(d.define(reserved, &definition.into())), IrErrorKind::InvalidDebugInfo);
    assert_eq!(kind(d.define(reserved, &DiNode::Undefined)), IrErrorKind::InvalidDebugInfo);
    assert_eq!(d.kind(reserved), DiKind::Undefined, "a refused definition defines nothing");
    d.define(reserved, &DiLexicalBlock { line: 4, ..Default::default() }.into()).unwrap();
    assert_eq!(kind(d.define(reserved, &file_node)), IrErrorKind::DebugRedefined);
    assert_eq!(kind(d.define_distinct(reserved, &file_node)), IrErrorKind::DebugRedefined);
    assert_eq!(d.kind(reserved), DiKind::LexicalBlock);
    d.check_defined().unwrap();
    m.finish_debug().unwrap();
}

/// §6.3: which nodes are one. Equal ids are the same node; a node added
/// as uniqued is found again by its content; a distinct node and a node
/// defined after its id was reserved never are.
#[test]
fn uniquing() {
    let Unit { mut m, file, int, .. } = unit();
    let d = &mut m.debug;
    let block = DiLexicalBlock { scope: None, file: Some(file), line: 4, column: 2 };
    let first = d.add(&block.into()).unwrap();
    let count = d.node_count();
    let stats = d.stats();
    assert_eq!(d.add(&block.into()).unwrap(), first);
    assert_eq!((d.node_count(), d.stats()), (count, stats), "a duplicate costs nothing");
    assert!(!d.is_distinct(first));
    // One field apart, in the value or in which field has it.
    for other in [
        DiLexicalBlock { line: 5, ..block },
        DiLexicalBlock { column: 0, ..block },
        DiLexicalBlock { line: 2, column: 4, ..block },
        DiLexicalBlock { scope: Some(file), file: None, ..block },
    ] {
        assert_ne!(d.add(&other.into()).unwrap(), first, "{other:?}");
    }
    // Another kind with the same fields, and another tag.
    let block_file = DiLexicalBlockFile { scope: None, file: Some(file), discriminator: 4 };
    assert_ne!(d.add(&block_file.into()).unwrap(), first);
    let pointer =
        DiDerivedType { tag: DW_TAG_POINTER_TYPE, base_type: Some(int), ..Default::default() };
    let reference = DiDerivedType { tag: hod_ir::DW_TAG_REFERENCE_TYPE, ..pointer };
    let pointer = d.add(&pointer.into()).unwrap();
    assert_ne!(d.add(&reference.into()).unwrap(), pointer);
    // Lists: by length and by element.
    let two = d.add(&DiSubroutineType { types: &[int, int], ..Default::default() }.into()).unwrap();
    let three =
        d.add(&DiSubroutineType { types: &[int, int, int], ..Default::default() }.into()).unwrap();
    let void = d
        .add(&DiSubroutineType { types: &[DiId::NONE, int], ..Default::default() }.into())
        .unwrap();
    assert!(two != three && two != void && three != void);
    assert_eq!(
        d.add(&DiSubroutineType { types: &[int, int, int], ..Default::default() }.into()).unwrap(),
        three
    );

    // Distinct nodes are never found, and never equal to each other.
    let distinct = d.add_distinct(&block.into()).unwrap();
    let again = d.add_distinct(&block.into()).unwrap();
    assert!(distinct != first && again != distinct && d.is_distinct(distinct));
    assert_eq!(d.add(&block.into()).unwrap(), first);
    // A node defined after its id was handed out keeps that id.
    let reserved = d.reserve().unwrap();
    d.define(reserved, &block.into()).unwrap();
    assert!(reserved != first && !d.is_distinct(reserved));
    assert_eq!(d.node(reserved), d.node(first));
    assert_eq!(d.add(&block.into()).unwrap(), first);
    // Ids count up in the order handed out.
    let next = d.add_distinct(&block.into()).unwrap();
    assert_eq!(next.index(), d.node_count() - 1);
    m.finish_debug().unwrap();
}

/// §6.3: a forward declaration with an identifier is completed once the
/// definition is known, and every reference then reads the definition.
#[test]
fn forward_declarations_are_completed() {
    let Unit { mut m, file, int, .. } = unit();
    let d = &mut m.debug;
    let declared = DiCompositeType {
        tag: DW_TAG_STRUCTURE_TYPE,
        name: b"S",
        flags: DiFlags::FWD_DECL,
        identifier: b"_ZTS1S",
        ..Default::default()
    };
    let id = d.add(&declared.into()).unwrap();
    let pointer = d
        .add(
            &DiDerivedType { tag: DW_TAG_POINTER_TYPE, base_type: Some(id), ..Default::default() }
                .into(),
        )
        .unwrap();
    let member = d
        .add(
            &DiDerivedType {
                tag: DW_TAG_MEMBER,
                name: b"next",
                scope: Some(id),
                base_type: Some(pointer),
                ..Default::default()
            }
            .into(),
        )
        .unwrap();
    let defined = DiCompositeType {
        flags: DiFlags::default(),
        file: Some(file),
        size: 64,
        elements: &[member],
        ..declared
    };
    // Not the same identifier; not a composite; not a declaration.
    let other = DiCompositeType { identifier: b"_ZTS1T", ..defined };
    assert_eq!(kind(d.complete(id, &other.into())), IrErrorKind::InvalidDebugInfo);
    assert_eq!(kind(d.complete(id, &DiFile::default().into())), IrErrorKind::InvalidDebugInfo);
    assert_eq!(kind(d.complete(int, &defined.into())), IrErrorKind::InvalidDebugInfo);
    assert_eq!(kind(d.complete(DiId::NONE, &defined.into())), IrErrorKind::UndefinedDebugNode);
    let dangling = DiCompositeType { elements: &[DiId::new(999)], ..defined };
    assert_eq!(kind(d.complete(id, &dangling.into())), IrErrorKind::UndefinedDebugNode);
    assert_eq!(d.node(id), DiNode::CompositeType(declared));

    d.complete(id, &defined.into()).unwrap();
    assert_eq!(d.node(id), DiNode::CompositeType(defined));
    assert!(!d.is_distinct(id));
    let DiNode::DerivedType(read) = d.node(pointer) else { panic!() };
    assert_eq!(read.base_type, Some(id), "the reference names the definition");
    assert_eq!(kind(d.complete(id, &defined.into())), IrErrorKind::InvalidDebugInfo, "once");
    // The declaration is no longer in the table: adding it again makes
    // a new node, which is not the definition.
    assert_ne!(d.add(&declared.into()).unwrap(), id);
    // A declaration without an identifier is not replaceable.
    let anonymous = DiCompositeType { identifier: b"", ..declared };
    let anonymous_id = d.add(&anonymous.into()).unwrap();
    let completion = DiCompositeType { identifier: b"", ..defined };
    assert_eq!(kind(d.complete(anonymous_id, &completion.into())), IrErrorKind::InvalidDebugInfo);
    m.finish_debug().unwrap();
    m.verify_debug().unwrap();
}

/// §6.4: expressions are interned lists of operations; an operation hod
/// has no member for is kept and reaches `classify` by name.
#[test]
fn expressions() {
    let mut m = module();
    let d = &mut m.debug;
    assert_eq!(d.expr(&[]).unwrap(), ExprId::EMPTY);
    assert_eq!(d.expr_from_elements(&[]).unwrap(), ExprId::EMPTY);
    assert!(d.is_empty(), "the empty expression is not debug information");
    assert_eq!(d.expr_count(), 1);
    assert_eq!(d.ops(ExprId::EMPTY).count(), 0);
    let ops = [
        DiOp::Constu(4),
        DiOp::Consts(-4i64 as u64),
        DiOp::Plus,
        DiOp::StackValue,
        DiOp::Fragment(32, 32),
    ];
    let id = d.expr(&ops).unwrap();
    assert_eq!(id, ExprId::new(1));
    assert_eq!(d.expr(&ops).unwrap(), id);
    let elements = [0x10, 4, 0x11, -4i64 as u64, 0x22, 0x9f, 0x1000, 32, 32];
    assert_eq!(d.expr_from_elements(&elements).unwrap(), id);
    assert_eq!(d.expr_elements(id), elements);
    assert_eq!(d.ops(id).collect::<Vec<_>>(), ops);
    assert_eq!(d.ops(ExprId::new(77)).count(), 0);
    assert!(!d.is_empty());
    assert_eq!(d.expr_count(), 2);
    assert_eq!(d.stats().expr_bytes, 9 * 8 + 2 * 8);
    // Operand counts are checked when the expression is made.
    for short in [&[0x10][..], &[0x06, 0x1000, 8], &[0x1001, 8]] {
        assert_eq!(kind(d.expr_from_elements(short)), IrErrorKind::InvalidDebugInfo);
    }
    assert_eq!(d.expr_count(), 2);
    m.finish_debug().unwrap();
    assert_eq!(m.unsupported().len(), 0);

    // DW_OP_regx 3, DW_OP_bregx-like unknowns, and a number LLVM has no
    // name for: kept, in order, and named once each.
    let d = &mut m.debug;
    let other = d.expr_from_elements(&[0x90, 3, 0x06, 0x1fff, 0x90]).unwrap();
    assert_eq!(
        d.ops(other).collect::<Vec<_>>(),
        [DiOp::Other(0x90), DiOp::Other(3), DiOp::Deref, DiOp::Other(0x1fff), DiOp::Other(0x90)]
    );
    m.finish_debug().unwrap();
    m.finish_debug().unwrap();
    let names: Vec<_> =
        m.unsupported().map(|record| (record.construct.to_string(), record.symbol)).collect();
    let expected = ["debug:DW_OP_regx", "debug:DW_OP_addr", "debug:DW_OP_0x1fff"];
    assert_eq!(names, expected.map(|name| (name.to_string(), None)));
    // Placement is `verify`'s.
    m.verify_debug().unwrap();
    m.debug.expr(&[DiOp::Fragment(0, 8), DiOp::Deref]).unwrap();
    assert_eq!(kind(m.verify_debug()), IrErrorKind::InvalidDebugInfo);
}

/// §6.5: locations are interned; a distinct one is not; chains of
/// inlined-at lead to the function an instruction is in.
#[test]
fn locations() {
    let Unit { mut m, file, unit, f, .. } = unit();
    let d = &mut m.debug;
    let block = d
        .add_distinct(
            &DiLexicalBlock { scope: Some(f), file: Some(file), line: 4, column: 3 }.into(),
        )
        .unwrap();
    let inner = d
        .add(&DiLexicalBlockFile { scope: Some(block), file: Some(file), discriminator: 1 }.into())
        .unwrap();
    assert_eq!(d.loc_count(), 0);
    let here = DiLocation { line: 5, column: 7, scope: inner, inlined_at: LocId::NONE };
    let a = d.loc(here).unwrap();
    assert_eq!(a, LocId::new(0));
    assert_eq!(d.loc(here).unwrap(), a);
    assert_eq!(d.location(a), Some(here));
    assert_eq!(d.loc_count(), 1);
    for other in [
        DiLocation { line: 6, ..here },
        DiLocation { column: 0, ..here },
        DiLocation { scope: block, ..here },
        DiLocation { inlined_at: a, ..here },
        DiLocation { line: 7, column: 5, ..here },
    ] {
        let id = d.loc(other).unwrap();
        assert_ne!(id, a);
        assert_eq!(d.location(id), Some(other));
        assert_eq!(d.loc(other).unwrap(), id);
    }
    assert_eq!(d.loc_count(), 6);
    assert_eq!(d.stats().loc_bytes, 6 * 16);
    // Scopes lead to the subprogram.
    assert_eq!(d.enclosing_block_scope(inner), Some(block));
    assert_eq!(d.enclosing_block_scope(block), Some(f));
    assert_eq!(d.enclosing_block_scope(f), None);
    assert_eq!(d.subprogram_of(inner), Some(f));
    assert_eq!(d.subprogram_of(f), Some(f));
    assert_eq!(d.subprogram_of(file), None);
    assert_eq!(d.subprogram_of(DiId::NONE), None);
    assert_eq!(d.function_of(a), Some(f));

    // Two calls of `g` on one line, and `h` inlined into `g`: the call
    // sites are distinct locations, and the chain is two deep.
    let callee = |d: &mut DebugInfo, name| {
        let node = DiSubprogram {
            name,
            sp_flags: SpFlags::DEFINITION,
            unit: Some(unit),
            ..Default::default()
        };
        d.add_distinct(&node.into()).unwrap()
    };
    let (g, h) = (callee(d, b"g"), callee(d, b"h"));
    let call = DiLocation { line: 9, column: 3, scope: f, inlined_at: LocId::NONE };
    let first_call = d.distinct_loc(call).unwrap();
    let second_call = d.distinct_loc(call).unwrap();
    assert_ne!(first_call, second_call);
    assert_ne!(d.loc(call).unwrap(), first_call, "a distinct location is never found");
    let in_g = d.loc(DiLocation { line: 20, column: 1, scope: g, inlined_at: first_call }).unwrap();
    let in_h = d.loc(DiLocation { line: 30, column: 1, scope: h, inlined_at: in_g }).unwrap();
    let in_h_again = d.loc(DiLocation { line: 30, column: 1, scope: h, inlined_at: in_g }).unwrap();
    assert_eq!(in_h, in_h_again);
    let other_g =
        d.loc(DiLocation { line: 20, column: 1, scope: g, inlined_at: second_call }).unwrap();
    assert_ne!(other_g, in_g, "the two inlined instances stay apart");
    let chain = d.location(in_h).unwrap();
    assert_eq!(d.subprogram_of(chain.scope), Some(h));
    let middle = d.location(chain.inlined_at).unwrap();
    assert_eq!((d.subprogram_of(middle.scope), middle.inlined_at), (Some(g), first_call));
    assert_eq!(d.location(middle.inlined_at).unwrap().inlined_at, LocId::NONE);
    assert_eq!(d.function_of(in_h), Some(f));
    assert_eq!(d.function_of(in_g), Some(f));

    // A location named before it is read.
    let early = d.reserve_loc().unwrap();
    assert_eq!(d.location(early), None);
    assert_eq!(d.function_of(early), None);
    let user = d.loc(DiLocation { line: 1, column: 1, scope: g, inlined_at: early }).unwrap();
    assert_eq!(kind(d.check_defined()), IrErrorKind::UndefinedDebugNode);
    d.define_loc(early, call).unwrap();
    assert_eq!(d.location(early), Some(call));
    assert_ne!(d.loc(call).unwrap(), early);
    assert_eq!(d.function_of(user), Some(f));
    assert_eq!(kind(d.define_loc(early, call)), IrErrorKind::DebugRedefined);
    assert_eq!(kind(d.define_loc(a, call)), IrErrorKind::DebugRedefined);
    assert_eq!(kind(d.define_loc(LocId::NONE, call)), IrErrorKind::UndefinedDebugNode);
    // Dangling scope and inlined-at.
    let count = d.loc_count();
    let no_scope = DiLocation { scope: DiId::NONE, ..here };
    let no_site = DiLocation { inlined_at: LocId::new(count as u32), ..here };
    for bad in [no_scope, no_site] {
        assert_eq!(kind(d.loc(bad)), IrErrorKind::UndefinedDebugNode);
        assert_eq!(kind(d.distinct_loc(bad)), IrErrorKind::UndefinedDebugNode);
    }
    let pending = d.reserve_loc().unwrap();
    assert_eq!(kind(d.define_loc(pending, no_scope)), IrErrorKind::UndefinedDebugNode);
    d.define_loc(pending, here).unwrap();
    assert_eq!(d.location(LocId::NONE), None);
    assert_eq!(d.loc_count(), count + 1);
    m.finish_debug().unwrap();
    m.verify_debug().unwrap();
}

/// §6.7: the compile-unit list, the unit's own fields, the two module
/// flags, and what is attached to symbols.
#[test]
fn attachments() {
    let Unit { mut m, file, unit, int, f } = unit();
    assert_eq!(m.debug.compile_units(), [unit]);
    let d = &mut m.debug;
    assert_eq!(kind(d.add_compile_unit(unit)), IrErrorKind::InvalidDebugInfo, "listed twice");
    assert_eq!(kind(d.add_compile_unit(file)), IrErrorKind::InvalidDebugInfo);
    assert_eq!(kind(d.add_compile_unit(DiId::NONE)), IrErrorKind::UndefinedDebugNode);
    // A unit that is not listed is a node like any other, and not emitted.
    let global = d
        .add_distinct(
            &DiGlobalVariable {
                name: b"g",
                scope: Some(unit),
                file: Some(file),
                ty: Some(int),
                is_definition: true,
                ..Default::default()
            }
            .into(),
        )
        .unwrap();
    let located = d
        .add(&DiGlobalVariableExpression { var: Some(global), expr: ExprId::EMPTY }.into())
        .unwrap();
    let high = d.expr(&[DiOp::Fragment(32, 32)]).unwrap();
    let part = d.add(&DiGlobalVariableExpression { var: Some(global), expr: high }.into()).unwrap();
    let retained = d
        .add(&DiBasicType { name: b"char", size: 8, encoding: 6, ..Default::default() }.into())
        .unwrap();
    let second = d
        .add_distinct(
            &DiCompileUnit {
                language: 0x21,
                file: Some(file),
                emission_kind: EmissionKind::LineTablesOnly,
                name_table_kind: NameTableKind::None,
                retained_types: &[retained],
                globals: &[located, part],
                ..Default::default()
            }
            .into(),
        )
        .unwrap();
    // A unit can be listed before it is defined.
    let third = d.reserve().unwrap();
    d.add_compile_unit(second).unwrap();
    d.add_compile_unit(third).unwrap();
    d.define_distinct(
        third,
        &DiCompileUnit { emission_kind: EmissionKind::NoDebug, ..Default::default() }.into(),
    )
    .unwrap();
    assert_eq!(d.compile_units(), [unit, second, third]);
    let DiNode::CompileUnit(read) = d.node(second) else { panic!() };
    assert_eq!(read.emission_kind, EmissionKind::LineTablesOnly);
    assert_eq!(read.name_table_kind, NameTableKind::None);
    assert_eq!((read.retained_types, read.globals), (&[retained][..], &[located, part][..]));
    assert!(read.imports.is_empty());

    // The module flags are module properties (§2.1).
    assert_eq!((m.props.dwarf_version, m.props.debug_info_version), (None, None));
    m.set_module_flag(7, b"Dwarf Version", FlagValue::Int(5)).unwrap();
    m.set_module_flag(2, b"Debug Info Version", FlagValue::Int(3)).unwrap();
    assert_eq!((m.props.dwarf_version, m.props.debug_info_version), (Some(5), Some(3)));

    // A function's subprogram; a variable's global-variable expressions.
    let fn_ty = m.types.function(TypeId::I32, &[TypeId::I32], false).unwrap();
    let func = m.symbol_named(b"f").unwrap();
    m.set_function(
        func,
        &FunctionDesc { subprogram: Some(f), has_body: true, ..FunctionDesc::new(fn_ty) },
    )
    .unwrap();
    let var = m.symbol_named(b"g").unwrap();
    // Attached before the symbol is described.
    m.attach_global_expr(var, located).unwrap();
    let zero = m.consts.int(&m.types, TypeId::I64, 0).unwrap();
    m.set_variable(var, &VariableDesc { init: Some(zero), ..VariableDesc::new(TypeId::I64) })
        .unwrap();
    m.attach_global_expr(var, part).unwrap();
    assert_eq!(m.global_exprs(), [(var, located), (var, part)]);
    assert_eq!(m.global_exprs_of(var).collect::<Vec<_>>(), [located, part]);
    assert_eq!(m.global_exprs_of(func).count(), 0);
    assert_eq!(kind(m.attach_global_expr(var, global)), IrErrorKind::InvalidDebugInfo);
    assert_eq!(kind(m.attach_global_expr(var, DiId::NONE)), IrErrorKind::UndefinedDebugNode);
    assert_eq!(kind(m.attach_global_expr(SymId::new(9), part)), IrErrorKind::UndefinedSymbol);
    assert_eq!(m.global_exprs().len(), 2);
    m.finish_debug().unwrap();
    m.verify_debug().unwrap();
    m.verify_symbols().unwrap();
}

/// §6.2, §6.3: a kind hod has no record for is a node that keeps the
/// graph whole, and `finish_debug` names it for `classify`, once.
#[test]
fn unknown_kinds_reach_classify() {
    let Unit { mut m, file, .. } = unit();
    let d = &mut m.debug;
    let string_type = d.add_distinct(&DiOther { kind: b"DIStringType" }.into()).unwrap();
    let again = d.add_distinct(&DiOther { kind: b"DIStringType" }.into()).unwrap();
    let module_node = d.add(&DiOther { kind: b"DIModule" }.into()).unwrap();
    assert_ne!(string_type, again);
    assert_eq!(d.node(module_node), DiNode::Other(DiOther { kind: b"DIModule" }));
    // It can be named where a type or a scope can.
    let member = DiDerivedType {
        tag: DW_TAG_MEMBER,
        scope: Some(module_node),
        base_type: Some(string_type),
        ..Default::default()
    };
    d.add(&member.into()).unwrap();
    m.verify_debug().unwrap();
    assert_eq!(m.unsupported().len(), 0, "recorded by finish_debug, not before");
    m.record_unsupported("attr:jumptable", None);
    m.finish_debug().unwrap();
    m.debug.add(&DiOther { kind: b"DICommonBlock" }.into()).unwrap();
    m.debug.add(&DiOther { kind: b"" }.into()).unwrap();
    m.finish_debug().unwrap();
    let names: Vec<_> = m.unsupported().map(|record| record.construct.to_string()).collect();
    assert_eq!(
        names,
        ["attr:jumptable", "debug:DIStringType", "debug:DIModule", "debug:DICommonBlock", "debug:"]
    );
    assert_eq!(
        hod_ir::di_kind(b"DIStringType"),
        Some(DiMapping::Unsupported("debug:DIStringType"))
    );
    // Not where LLVM requires one particular kind.
    let block = DiLexicalBlock { file: Some(module_node), ..Default::default() };
    m.debug.add(&block.into()).unwrap();
    assert_eq!(kind(m.verify_debug()), IrErrorKind::InvalidDebugInfo);
    let _ = file;
}

/// What `verify_debug` says of the unit after `damage`.
fn verdict(damage: impl FnOnce(&mut Module, &Unit2)) -> Option<IrErrorKind> {
    let Unit { mut m, file, unit, int, f } = unit();
    let fn_ty = m.types.function(TypeId::VOID, &[], false).unwrap();
    let func = m.symbol_named(b"f").unwrap();
    let desc = FunctionDesc { subprogram: Some(f), has_body: true, ..FunctionDesc::new(fn_ty) };
    m.set_function(func, &desc).unwrap();
    damage(&mut m, &Unit2 { file, unit, int, f, fn_ty });
    m.verify_debug().err().map(|error| error.kind)
}

struct Unit2 {
    file: DiId,
    unit: DiId,
    int: DiId,
    f: DiId,
    fn_ty: TypeId,
}

/// §6.8: the rules of `Module::verify_debug`, each with a table that
/// keeps it and one that breaks it.
#[test]
#[allow(clippy::too_many_lines)]
fn verify_accepts_and_rejects() {
    use IrErrorKind::{InvalidDebugInfo, UndefinedDebugNode};
    assert_eq!(verdict(|_, _| {}), None);
    assert_eq!(module().verify_debug(), Ok(()), "no debug information is well formed");

    // Everything reserved is defined.
    assert_eq!(
        verdict(|m, _| {
            m.debug.reserve().unwrap();
        }),
        Some(UndefinedDebugNode)
    );
    assert_eq!(
        verdict(|m, _| {
            m.debug.reserve_loc().unwrap();
        }),
        Some(UndefinedDebugNode)
    );

    // A reference names a node of a kind its field allows.
    let add = |m: &mut Module, node: DiNode<'_>| m.debug.add(&node).unwrap();
    let put = |m: &mut Module, node: DiNode<'_>| {
        m.debug.add(&node).unwrap();
    };
    assert_eq!(
        verdict(|m, u| put(
            m,
            DiLexicalBlock { scope: Some(u.f), file: Some(u.file), ..Default::default() }.into()
        )),
        None
    );
    assert_eq!(
        verdict(|m, u| put(
            m,
            DiLexicalBlock { scope: Some(u.f), file: Some(u.f), ..Default::default() }.into()
        )),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, u| put(m, DiLexicalBlock { scope: Some(u.file), ..Default::default() }.into())),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, u| put(
            m,
            DiLocalVariable { name: b"v", scope: Some(u.f), ty: Some(u.int), ..Default::default() }
                .into()
        )),
        None
    );
    assert_eq!(
        verdict(|m, u| put(
            m,
            DiLocalVariable {
                name: b"v",
                scope: Some(u.f),
                ty: Some(u.file),
                ..Default::default()
            }
            .into()
        )),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, u| put(
            m,
            DiLocalVariable { name: b"v", scope: Some(u.unit), ..Default::default() }.into()
        )),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, u| put(
            m,
            DiLabel { name: b"l", scope: Some(u.int), ..Default::default() }.into()
        )),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, u| put(
            m,
            DiSubroutineType { types: &[DiId::NONE, u.int, DiId::NONE], ..Default::default() }
                .into()
        )),
        None
    );
    assert_eq!(
        verdict(|m, u| put(
            m,
            DiSubroutineType { types: &[u.int, u.unit], ..Default::default() }.into()
        )),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, u| put(m, DiSubprogram { unit: Some(u.file), ..Default::default() }.into())),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, u| put(
            m,
            DiSubprogram { declaration: Some(u.int), ..Default::default() }.into()
        )),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, u| put(
            m,
            DiGlobalVariableExpression { var: Some(u.f), expr: ExprId::EMPTY }.into()
        )),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, u| put(
            m,
            DiCompositeType {
                tag: DW_TAG_STRUCTURE_TYPE,
                elements: &[u.f, u.int],
                vtable_holder: Some(u.int),
                ..Default::default()
            }
            .into()
        )),
        None
    );
    assert_eq!(
        verdict(|m, u| put(
            m,
            DiCompositeType {
                tag: DW_TAG_STRUCTURE_TYPE,
                template_params: &[u.int],
                ..Default::default()
            }
            .into()
        )),
        Some(InvalidDebugInfo)
    );

    // A constant used as metadata is a constant of the module.
    assert_eq!(
        verdict(|m, _| {
            let value = m.consts.int(&m.types, TypeId::I8, 1).unwrap();
            put(m, DiConstant { value }.into());
        }),
        None
    );
    assert_eq!(
        verdict(|m, _| put(m, DiConstant { value: hod_ir::ConstId::new(1 << 20) }.into())),
        Some(InvalidDebugInfo)
    );

    // A listed compile unit is one.
    assert_eq!(
        verdict(|m, u| {
            let later = m.debug.reserve().unwrap();
            m.debug.add_compile_unit(later).unwrap();
            m.debug.define(later, &DiFile::default().into()).unwrap();
            let _ = u;
        }),
        Some(InvalidDebugInfo)
    );

    // A location's scope is a subprogram or a block of one.
    let loc = |scope| DiLocation { line: 1, column: 1, scope, inlined_at: LocId::NONE };
    assert_eq!(
        verdict(|m, u| {
            m.debug.loc(loc(u.f)).unwrap();
        }),
        None
    );
    assert_eq!(
        verdict(|m, u| {
            m.debug.loc(loc(u.file)).unwrap();
        }),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, u| {
            m.debug.loc(loc(u.unit)).unwrap();
        }),
        Some(InvalidDebugInfo)
    );
    // ... whose chain of blocks ends at a subprogram.
    assert_eq!(
        verdict(|m, _| {
            let orphan = add(m, DiLexicalBlock { line: 9, ..Default::default() }.into());
            m.debug.loc(loc(orphan)).unwrap();
        }),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, _| {
            let a = m.debug.reserve().unwrap();
            let b = add(m, DiLexicalBlock { scope: Some(a), ..Default::default() }.into());
            m.debug
                .define(a, &DiLexicalBlock { scope: Some(b), ..Default::default() }.into())
                .unwrap();
            m.debug.loc(loc(a)).unwrap();
        }),
        Some(InvalidDebugInfo)
    );
    // ... and whose inlined-at chain ends.
    assert_eq!(
        verdict(|m, u| {
            let a = m.debug.reserve_loc().unwrap();
            let b = m.debug.loc(DiLocation { inlined_at: a, ..loc(u.f) }).unwrap();
            m.debug.define_loc(a, DiLocation { inlined_at: b, ..loc(u.f) }).unwrap();
        }),
        Some(InvalidDebugInfo)
    );

    // An expression's operations are placed as LLVM requires.
    assert_eq!(
        verdict(|m, _| {
            m.debug.expr(&[DiOp::EntryValue(1), DiOp::StackValue]).unwrap();
        }),
        None
    );
    assert_eq!(
        verdict(|m, _| {
            m.debug.expr(&[DiOp::StackValue, DiOp::Deref]).unwrap();
        }),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, _| {
            m.debug.expr(&[DiOp::Swap]).unwrap();
        }),
        Some(InvalidDebugInfo)
    );

    // A function's subprogram is a subprogram; a defined function's is
    // distinct and its own.
    let function = |m: &mut Module, u: &Unit2, name: &[u8], subprogram, has_body| {
        let sym = m.symbol_named(name).unwrap();
        let desc =
            FunctionDesc { subprogram: Some(subprogram), has_body, ..FunctionDesc::new(u.fn_ty) };
        m.set_function(sym, &desc).unwrap();
    };
    assert_eq!(verdict(|m, u| function(m, u, b"g", u.file, false)), Some(InvalidDebugInfo));
    assert_eq!(
        verdict(|m, u| function(m, u, b"g", DiId::new(4000), false)),
        Some(InvalidDebugInfo)
    );
    assert_eq!(verdict(|m, u| function(m, u, b"g", u.f, true)), Some(InvalidDebugInfo), "f has it");
    assert_eq!(verdict(|m, u| function(m, u, b"g", u.f, false)), None, "a declaration may share");
    assert_eq!(
        verdict(|m, u| {
            let declaration = add(m, DiSubprogram { name: b"g", ..Default::default() }.into());
            function(m, u, b"g", declaration, false);
        }),
        None
    );
    assert_eq!(
        verdict(|m, u| {
            let declaration = add(m, DiSubprogram { name: b"g", ..Default::default() }.into());
            function(m, u, b"g", declaration, true);
        }),
        Some(InvalidDebugInfo)
    );

    // A global-variable expression is attached to a variable.
    let gve = |m: &mut Module, u: &Unit2| {
        let var = m
            .debug
            .add_distinct(
                &DiGlobalVariable { name: b"v", ty: Some(u.int), ..Default::default() }.into(),
            )
            .unwrap();
        add(m, DiGlobalVariableExpression { var: Some(var), expr: ExprId::EMPTY }.into())
    };
    assert_eq!(
        verdict(|m, u| {
            let expr = gve(m, u);
            let sym = m.symbol_named(b"v").unwrap();
            m.set_variable(sym, &VariableDesc::new(TypeId::I32)).unwrap();
            m.attach_global_expr(sym, expr).unwrap();
        }),
        None
    );
    assert_eq!(
        verdict(|m, u| {
            let expr = gve(m, u);
            let sym = m.symbol_named(b"f").unwrap();
            m.attach_global_expr(sym, expr).unwrap();
        }),
        Some(InvalidDebugInfo)
    );
    assert_eq!(
        verdict(|m, _| {
            let later = m.debug.reserve().unwrap();
            let sym = m.symbol_named(b"v").unwrap();
            m.set_variable(sym, &VariableDesc::new(TypeId::I32)).unwrap();
            m.attach_global_expr(sym, later).unwrap();
            m.debug.define(later, &DiFile::default().into()).unwrap();
        }),
        Some(InvalidDebugInfo)
    );
}

/// §6.10: what a stored node costs: eight bytes, four for the mask, and
/// four for each cell of a field that is not at its default. No field of
/// a node is a heap allocation of its own.
#[test]
fn node_sizes() {
    let Unit { mut m, file, unit, int, f } = unit();
    let d = &mut m.debug;
    let mut cost = |node: DiNode<'_>| {
        let before = d.stats();
        let id = d.add_distinct(&node).unwrap();
        assert_eq!(d.node(id), node);
        assert_eq!(d.stats().nodes, before.nodes + 1);
        d.stats().node_bytes - before.node_bytes
    };
    // A file: two strings.
    assert_eq!(
        cost(DiFile { filename: b"b.c", directory: b"/src", ..Default::default() }.into()),
        20
    );
    // A local variable as clang writes it: name, scope, file, line, type.
    let local = DiLocalVariable {
        name: b"x",
        scope: Some(f),
        file: Some(file),
        line: 4,
        ty: Some(int),
        ..Default::default()
    };
    assert_eq!(cost(local.into()), 32);
    assert_eq!(cost(DiLocalVariable { arg: 1, ..local }.into()), 36);
    // A lexical block: scope, file, line, column.
    assert_eq!(
        cost(DiLexicalBlock { scope: Some(f), file: Some(file), line: 4, column: 9 }.into()),
        28
    );
    // A pointer type: base type and a 64-bit size (two cells).
    let pointer = DiDerivedType {
        tag: DW_TAG_POINTER_TYPE,
        base_type: Some(int),
        size: 64,
        ..Default::default()
    };
    assert_eq!(cost(pointer.into()), 24);
    // A member: name, scope, file, line, base type, size, offset.
    let member = DiDerivedType {
        tag: DW_TAG_MEMBER,
        name: b"m",
        scope: Some(int),
        file: Some(file),
        line: 2,
        offset: 32,
        ..pointer
    };
    assert_eq!(cost(member.into()), 48);
    // A list costs its length and its elements; a flag costs nothing.
    assert_eq!(cost(DiSubroutineType { types: &[int, int, int], ..Default::default() }.into()), 28);
    assert_eq!(cost(DiNamespace { name: b"n", scope: None, export_symbols: true }.into()), 16);
    assert_eq!(cost(DiNamespace { name: b"n", scope: None, export_symbols: false }.into()), 16);
    // A subprogram definition as clang writes it, eleven fields.
    let definition = DiSubprogram {
        name: b"method",
        linkage_name: b"_ZN1C6methodEv",
        scope: Some(file),
        file: Some(file),
        line: 10,
        ty: Some(int),
        scope_line: 10,
        flags: DiFlags::PROTOTYPED,
        sp_flags: SpFlags::DEFINITION,
        unit: Some(unit),
        retained_nodes: &[int],
        ..Default::default()
    };
    assert_eq!(cost(definition.into()), 8 + 4 + 4 * 10 + 4 * 2);
    // A subrange bound is three cells.
    assert_eq!(cost(DiSubrange { count: DiBound::Const(4), ..Default::default() }.into()), 24);
    // The views are small enough to pass by value and hold no allocation.
    assert!(size_of::<DiNode<'_>>() <= 176, "{}", size_of::<DiNode<'_>>());
    assert_eq!(size_of::<DiLocation>(), 16);
    assert_eq!(size_of::<DiId>(), 4);
    assert_eq!(size_of::<ExprId>(), 4);
    assert_eq!(size_of::<LocId>(), 4);
    // Strings are stored once.
    let before = d.stats().string_bytes;
    d.add_distinct(&DiFile { filename: b"b.c", directory: b"/src", ..Default::default() }.into())
        .unwrap();
    assert_eq!(d.stats().string_bytes, before);
    d.add_distinct(&DiFile { filename: b"new.c", ..Default::default() }.into()).unwrap();
    assert_eq!(d.stats().string_bytes, before + 5 + 8);
}

/// §6.1: a module without debug information has debug tables that hold
/// and have allocated nothing, and its checks are two compares.
#[test]
fn no_debug_information_costs_nothing() {
    let mut m = module();
    assert!(m.debug.is_empty());
    assert_eq!(m.debug.stats(), DebugStats { exprs: 1, ..DebugStats::default() });
    assert_eq!((m.debug.node_count(), m.debug.loc_count(), m.debug.expr_count()), (0, 0, 1));
    assert!(m.debug.compile_units().is_empty() && m.global_exprs().is_empty());
    assert_eq!(m.debug.location(LocId::new(0)), None);
    assert_eq!(m.debug.find_kind(DiId::new(0)), None);
    m.finish_debug().unwrap();
    m.verify_debug().unwrap();
    assert_eq!(m.unsupported().len(), 0);
    assert!(m.debug.is_empty());
    // The whole of the tables in a module: a fixed number of words.
    assert!(size_of::<DebugInfo>() <= 400, "{}", size_of::<DebugInfo>());
}

/// The field table as `spec/landed/ir/WP-005.md` prints it.
fn printed_field_table() -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let what = |mapping: DiMapping| match mapping {
        DiMapping::Mapped => ("mapped", String::new()),
        DiMapping::Dropped(why) => ("dropped", why.to_string()),
        DiMapping::Unsupported(name) => ("unsupported", format!("`{name}`")),
    };
    out.push_str("| kind | mapped fields |\n|---|---|\n");
    for kind in DiKind::ALL {
        let fields: Vec<_> = kind.fields().iter().map(|field| format!("`{field}`")).collect();
        writeln!(out, "| `{}` | {} |", kind.llvm_name(), fields.join(" ")).unwrap();
    }
    out.push_str("\n| field | becomes | why, or the construct name |\n|---|---|---|\n");
    for (kind, field, mapping) in DI_FIELD_NOTES {
        let (becomes, note) = what(*mapping);
        writeln!(out, "| `{kind}.{field}` | {becomes} | {note} |").unwrap();
    }
    out.push_str("\n| kind or record | becomes | why, or the construct name |\n|---|---|---|\n");
    for (name, mapping) in DI_KIND_NOTES.iter().chain(DI_RECORDS) {
        let (becomes, note) = what(*mapping);
        writeln!(out, "| `{name}` | {becomes} | {note} |").unwrap();
    }
    out.push_str("\n| operation | code | operands |\n|---|---|---|\n");
    for (name, code, operands) in DI_OPS {
        writeln!(out, "| `{name}` | {code:#x} | {operands} |").unwrap();
    }
    out
}

/// §6.9: the field table printed in `spec/landed/ir/WP-005.md` is the
/// one in the crate.
#[test]
fn the_landed_field_table_is_current() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/landed/ir/WP-005.md");
    let landed = std::fs::read_to_string(&path).unwrap_or_default();
    let printed = landed
        .split_once("<!-- field table: begin -->\n")
        .and_then(|(_, rest)| rest.split_once("<!-- field table: end -->"))
        .map(|(table, _)| table);
    let expected = printed_field_table();
    if printed != Some(expected.as_str()) {
        let fresh = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("debug-field-table.md");
        std::fs::write(&fresh, &expected).unwrap();
        panic!("{} does not print the field table; it is in {}", path.display(), fresh.display());
    }
}
