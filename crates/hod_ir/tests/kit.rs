//! `spec/ir.md` "Producer kit": what each helper of `hod_ir::kit`
//! promises a producer. The readers' byte equality over the corpus is
//! the end-to-end check (WP-092's record); these pin the contract.

use hod_ir::kit::{
    AllocaDesc, AllocaError, AttrUse, BranchUpgrade, Gep, GepError, GlobalAttachment, Lowered,
    NamedMetadataKind, SpecialError, SpecialGlobal, TypedAttrError, ValueKeyword, add_typed,
    annotations_construct, attr_construct, deliver_special, fn_keyword, gep_index_value,
    lower_alloca, module_flag, string_attr, value_keyword,
};
use hod_ir::kit::{
    DEBUG_INVALID, DebugRecord, DebugRecordKind, add_location, compile_unit,
    debug_intrinsic_construct, define_di_node, define_di_tuple, di_atom, di_node,
    emit_debug_record, finish_debug, function_subprogram, invalid_di_ref, unsupported_field,
    unsupported_field_construct, unsupported_record_construct, variable_expression,
};
use hod_ir::{
    AttrClass, FnAttrs, FunctionBuilder, InstKind, Module, ParamAttrs, ParamMem, SymId, TargetSpec,
    TypeId, Value,
};
use hod_ir::{DiAtom, DiFieldValue, DiId, DiKind, DiLocation, DiNode, LocId};

const I8: TypeId = TypeId::I8;
const I32: TypeId = TypeId::I32;
const I64: TypeId = TypeId::I64;
const PTR: TypeId = TypeId::PTR;

/// A module and a builder of `void f(i64, ptr)` positioned in its entry
/// block.
fn fixture() -> (Module, FunctionBuilder) {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let fn_ty = m.types.function(TypeId::VOID, &[I64, PTR], false).unwrap();
    let mut b = FunctionBuilder::new(&m, SymId::new(0), fn_ty).unwrap();
    let entry = b.create_block().unwrap();
    b.begin_block(entry).unwrap();
    (m, b)
}

fn int(m: &mut Module, ty: TypeId, value: u64) -> Value {
    Value::constant(m.consts.int(&m.types, ty, value).unwrap()).unwrap()
}

/// The `PtrAdd` the builder made last, as (offset, scales).
fn last_ptr_add(m: &Module, b: FunctionBuilder) -> (i64, Vec<u64>) {
    let mut b = b;
    b.ret(None).unwrap();
    let body = b.finish().unwrap();
    body.verify(m).unwrap();
    let mut found = body.insts().filter_map(|inst| match inst.kind() {
        InstKind::PtrAdd(add) => {
            Some((add.offset, add.indices().map(|(_, scale)| scale).collect::<Vec<_>>()))
        }
        _ => None,
    });
    found.next_back().expect("a PtrAdd")
}

#[test]
fn gep_folds_constant_indices_and_scales_variable_ones() {
    // getelementptr { i8, [4 x { i32, i64 }] }, ptr %p, i64 1, i32 1, i64 %n, i32 1
    let (mut m, mut b) = fixture();
    let inner = m.types.struct_type(&[I32, I64], false).unwrap();
    let array = m.types.array(inner, 4).unwrap();
    let outer = m.types.struct_type(&[I8, array], false).unwrap();
    let (n, p) = (b.arg(0).unwrap(), b.arg(1).unwrap());
    let mut gep = Gep::new(&m.types, outer, PTR).unwrap();
    let mut indices = Vec::new();
    for (ty, value, constant) in [
        (I64, Value::NONE, Some(1)),
        (I32, Value::NONE, Some(1)),
        (I64, n, None),
        (I32, Value::NONE, Some(1)),
    ] {
        assert_eq!(gep.index_type(&m.types, ty).unwrap(), m.types.int_bits(ty).unwrap());
        gep.index(&m.types, value, constant, &mut indices).unwrap();
    }
    let outer_size = m.types.alloc_size_of(outer) as i64;
    let at_array = m.types.field_offset(outer, 1) as i64;
    let at_field = m.types.field_offset(inner, 1) as i64;
    assert_eq!(gep.offset(), outer_size + at_array + at_field);
    assert_eq!(indices, vec![(n, m.types.alloc_size_of(inner))]);
    let (ty, lowered) = gep.build(&mut m, &mut b, p, &indices).unwrap();
    assert_eq!(ty, PTR);
    assert!(matches!(lowered, Lowered::Built(_)));
    let (offset, scales) = last_ptr_add(&m, b);
    assert_eq!(offset, outer_size + at_array + at_field);
    assert_eq!(scales, vec![m.types.alloc_size_of(inner)]);
}

#[test]
fn gep_index_values_are_sign_extended_and_splats_read_through() {
    let mut m = Module::new(TargetSpec::x86_64_linux_gnu());
    let minus_one = m.consts.int(&m.types, TypeId::I8, 0xff).unwrap();
    assert_eq!(gep_index_value(&m.types, &m.consts, minus_one), Some(-1));
    let v4 = m.types.vector(I32, 4).unwrap();
    let three = m.consts.int(&m.types, I32, 3).unwrap();
    let splat = m.consts.aggregate(&m.types, v4, &[three; 4]).unwrap();
    assert_eq!(gep_index_value(&m.types, &m.consts, splat), Some(3));
    let one = m.consts.int(&m.types, I32, 1).unwrap();
    let mixed = m.consts.aggregate(&m.types, v4, &[three, one, three, three]).unwrap();
    assert_eq!(gep_index_value(&m.types, &m.consts, mixed), None);
}

#[test]
fn gep_errors_are_llvms() {
    let (mut m, _) = fixture();
    let st = m.types.struct_type(&[I32, I64], false).unwrap();
    assert_eq!(Gep::new(&m.types, st, I64).unwrap_err(), GepError::BaseNotPointer);
    let opaque = m.types.opaque_struct().unwrap();
    assert_eq!(Gep::new(&m.types, opaque, PTR).unwrap_err(), GepError::SourceUnsized);
    let mut gep = Gep::new(&m.types, st, PTR).unwrap();
    assert_eq!(gep.index_type(&m.types, PTR).unwrap_err(), GepError::IndexNotInteger);
    let mut indices = Vec::new();
    gep.index(&m.types, Value::NONE, Some(0), &mut indices).unwrap();
    // A struct index must be a constant in range.
    assert_eq!(
        gep.index(&m.types, Value::NONE, Some(2), &mut indices).unwrap_err(),
        GepError::InvalidIndices
    );
    let v2 = m.types.vector(I64, 2).unwrap();
    let v4 = m.types.vector(I64, 4).unwrap();
    let mut gep = Gep::new(&m.types, I32, PTR).unwrap();
    gep.index_type(&m.types, v2).unwrap();
    assert_eq!(gep.index_type(&m.types, v4).unwrap_err(), GepError::VectorLength);
    let vp = gep.result_type(&mut m.types).unwrap();
    assert_eq!(vp, m.types.vector(PTR, 2).unwrap());
}

#[test]
fn gep_into_a_vector_of_bits_is_refused() {
    let (mut m, mut b) = fixture();
    let v8 = m.types.vector(TypeId::I1, 8).unwrap();
    let mut gep = Gep::new(&m.types, v8, PTR).unwrap();
    let mut indices = Vec::new();
    gep.index(&m.types, Value::NONE, Some(0), &mut indices).unwrap();
    gep.index(&m.types, Value::NONE, Some(3), &mut indices).unwrap();
    let p = b.arg(1).unwrap();
    let (_, lowered) = gep.build(&mut m, &mut b, p, &indices).unwrap();
    assert_eq!(lowered.refused(), Some("getelementptr:vector-element-not-bytes"));
}

fn alloca(ty: TypeId) -> AllocaDesc {
    AllocaDesc {
        ty,
        count: None,
        align: None,
        addr_space: 0,
        in_entry_block: true,
        in_alloca: false,
        swift_error: false,
    }
}

#[test]
fn a_static_alloca_is_a_slot_and_any_other_an_alloca() {
    let (mut m, mut b) = fixture();
    let arr = m.types.array(I32, 3).unwrap();
    let count = int(&mut m, I64, 2);
    let desc = AllocaDesc { count: Some(count), align: Some(16), ..alloca(arr) };
    let (ty, slot) = lower_alloca(&mut m, &mut b, &desc).unwrap();
    assert_eq!(ty, PTR);
    assert!(
        matches!(slot, Lowered::Built(value) if b.slot(hod_ir::SlotId::new(0)).unwrap() == value)
    );
    // Outside the entry block, or with a variable count: the instruction.
    let n = b.arg(0).unwrap();
    let (_, dynamic) =
        lower_alloca(&mut m, &mut b, &AllocaDesc { count: Some(n), ..alloca(I64) }).unwrap();
    let (_, late) =
        lower_alloca(&mut m, &mut b, &AllocaDesc { in_entry_block: false, ..alloca(I64) }).unwrap();
    assert!(matches!(dynamic, Lowered::Built(_)) && matches!(late, Lowered::Built(_)));
    b.ret(None).unwrap();
    let body = b.finish().unwrap();
    body.verify(&m).unwrap();
    assert_eq!(body.slots().len(), 1);
    assert_eq!(body.slots()[0].size(), 2 * 12);
    let allocas =
        body.insts().filter(|inst| matches!(inst.kind(), InstKind::Alloca { .. })).count();
    assert_eq!(allocas, 2);
}

#[test]
fn alloca_refusals_come_in_order_after_the_type_check() {
    let (mut m, mut b) = fixture();
    assert_eq!(
        lower_alloca(&mut m, &mut b, &alloca(TypeId::TOKEN)).unwrap_err(),
        AllocaError::InvalidType
    );
    let both = AllocaDesc { in_alloca: true, swift_error: true, ..alloca(I32) };
    let (_, lowered) = lower_alloca(&mut m, &mut b, &both).unwrap();
    assert_eq!(lowered.refused(), Some("alloca:inalloca"));
    let swift = AllocaDesc { swift_error: true, ..alloca(PTR) };
    let (_, lowered) = lower_alloca(&mut m, &mut b, &swift).unwrap();
    assert_eq!(lowered.refused(), Some("alloca:swifterror"));
    let scalable = m.types.scalable_vector(I32, 4).unwrap();
    let (_, lowered) = lower_alloca(&mut m, &mut b, &alloca(scalable)).unwrap();
    assert_eq!(lowered.refused(), Some("alloca:scalable"));
}

#[test]
fn attributes_map_as_the_table_says() {
    let mut fa = FnAttrs::new();
    assert_eq!(fn_keyword(&mut fa, b"noreturn", AttrClass::Kept).unwrap(), AttrUse::Kept);
    assert_ne!(fa.flags() & FnAttrs::NORETURN, 0);
    assert_eq!(fn_keyword(&mut fa, b"noinline", AttrClass::Dropped).unwrap(), AttrUse::Dropped);
    assert_eq!(fn_keyword(&mut fa, b"zeroext", AttrClass::Kept).unwrap(), AttrUse::Misplaced);
    assert_eq!(string_attr(Some(&mut fa), b"frame-pointer", b"all").unwrap(), AttrUse::Kept);
    assert_eq!(string_attr(None, b"frame-pointer", b"all").unwrap(), AttrUse::Rejected);
    assert_eq!(string_attr(None, b"no-such-attribute", b"").unwrap(), AttrUse::Rejected);
    assert_eq!(attr_construct(b"x"), "attr:x");
    assert_eq!(value_keyword(b"signext"), ValueKeyword::Flag(ParamAttrs::SIGNEXT));
    assert_eq!(value_keyword(b"byval"), ValueKeyword::Typed(ParamMem::ByVal));
    assert_eq!(value_keyword(b"align"), ValueKeyword::Align);
    assert_eq!(value_keyword(b"noundef"), ValueKeyword::Other);
}

#[test]
fn elementtype_beside_another_typed_attribute_keeps_the_other() {
    let sret = ParamAttrs::NONE.with_mem(ParamMem::Sret, I32).unwrap();
    let (kept, construct) = add_typed(sret, ParamMem::ElementType, I64).unwrap();
    assert_eq!((kept.mem(), construct), (ParamMem::Sret, Some("attr:elementtype-with-sret")));
    let elem = ParamAttrs::NONE.with_mem(ParamMem::ElementType, I64).unwrap();
    let (kept, construct) = add_typed(elem, ParamMem::ByVal, I32).unwrap();
    assert_eq!((kept.mem(), construct), (ParamMem::ByVal, Some("attr:elementtype-with-byval")));
    assert_eq!(add_typed(sret, ParamMem::ByVal, I32).unwrap_err(), TypedAttrError::TwoTyped);
}

#[test]
fn special_arrays_are_delivered_into_the_module() {
    let mut m = Module::new(TargetSpec::x86_64_linux_gnu());
    let f = m.symbol_named(b"f").unwrap();
    let addr = m.consts.addr(f, 0).unwrap();
    let null = m.consts.null(&m.types, PTR).unwrap();
    let entry_ty = m.types.struct_type(&[I32, PTR, PTR], false).unwrap();
    let prio = m.consts.int(&m.types, I32, 101).unwrap();
    let entry = m.consts.aggregate(&m.types, entry_ty, &[prio, addr, null]).unwrap();
    let skipped = m.consts.aggregate(&m.types, entry_ty, &[prio, null, null]).unwrap();
    let ctors_ty = m.types.array(entry_ty, 2).unwrap();
    let ctors = m.consts.aggregate(&m.types, ctors_ty, &[entry, skipped]).unwrap();
    deliver_special(&mut m, SpecialGlobal::Ctors, ctors).unwrap();
    assert_eq!(m.ctors().len(), 1);
    assert_eq!((m.ctors()[0].priority, m.ctors()[0].func), (101, f));
    // An array of nulls only is a zero array, which has no members.
    let used_ty = m.types.array(PTR, 2).unwrap();
    let with_null = m.consts.aggregate(&m.types, used_ty, &[addr, null]).unwrap();
    let error = deliver_special(&mut m, SpecialGlobal::Used, with_null).unwrap_err();
    assert_eq!(error, SpecialError::NullUsed.into());
    assert_eq!(SpecialGlobal::from_name(b"llvm.compiler.used"), Some(SpecialGlobal::CompilerUsed));
    assert_eq!(SpecialGlobal::from_name(b"llvm.usedx"), None);
    assert_eq!(annotations_construct(Some(b"llvm.metadata")), None);
    assert!(annotations_construct(None).is_some());
}

#[test]
fn metadata_names_and_branch_protection_flags() {
    assert_eq!(GlobalAttachment::of(b"dbg"), GlobalAttachment::Debug);
    assert_eq!(GlobalAttachment::of(b"type"), GlobalAttachment::Dropped);
    assert_eq!(GlobalAttachment::of(b"absolute_symbol"), GlobalAttachment::Unsupported);
    assert_eq!(NamedMetadataKind::of(b"llvm.module.flags"), NamedMetadataKind::ModuleFlags);
    assert_eq!(NamedMetadataKind::of(b"llvm.ident"), NamedMetadataKind::Dropped);
    assert_eq!(NamedMetadataKind::of(b"llvm.foo"), NamedMetadataKind::Unsupported);
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let mut upgrade = Some(BranchUpgrade::default());
    module_flag(&mut m, 8, b"branch-target-enforcement", Some(1), None, &mut upgrade).unwrap();
    assert_eq!(upgrade, Some(BranchUpgrade { bte: true, ..BranchUpgrade::default() }));
    // The value 2 is the current form: no upgrade.
    module_flag(&mut m, 8, b"sign-return-address", Some(2), None, &mut upgrade).unwrap();
    assert_eq!(upgrade, None);
}

fn noted(m: &Module, construct: &str) -> bool {
    m.unsupported().any(|u| u.construct == construct)
}

/// §12 debug fields: a node is LLVM's defaults with the fields read set;
/// a kind `hod_ir` does not represent is `Other`; a tuple and the
/// stand-in for an invalid reference are tuples.
#[test]
fn debug_nodes_are_built_over_llvm_defaults() {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let fields = [("name", DiFieldValue::Str(b"int")), ("size", DiFieldValue::UInt(32))];
    let node = di_node(Some(DiKind::BasicType), b"", fields).unwrap();
    let id = m.debug.reserve().unwrap();
    define_di_node(&mut m.debug, id, &node, false).unwrap();
    assert_eq!(m.debug.find_kind(id), Some(DiKind::BasicType));
    assert!(matches!(di_node(None, b"DIModule", []).unwrap(), DiNode::Other(_)));
    let tuple = m.debug.reserve().unwrap();
    define_di_tuple(&mut m.debug, tuple, &[id, DiId::NONE]).unwrap();
    assert_eq!(m.debug.find_kind(tuple), Some(DiKind::Tuple));
    let stand_in = invalid_di_ref(&mut m.debug).unwrap();
    assert_eq!(m.debug.find_kind(stand_in), Some(DiKind::Tuple));
    assert_eq!(invalid_di_ref(&mut m.debug).unwrap(), stand_in, "uniqued");
}

/// An unsupported field is recorded only when it is not the default, by
/// the mapping's construct, else `debug:<kind>.<field>`.
#[test]
fn unsupported_fields_are_recorded_unless_default() {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let construct = unsupported_field_construct(b"DICompositeType", b"dataLocation");
    unsupported_field(&mut m, b"DICompositeType", b"dataLocation", true);
    assert!(!noted(&m, &construct));
    unsupported_field(&mut m, b"DICompositeType", b"dataLocation", false);
    assert!(noted(&m, &construct));
    assert_eq!(unsupported_field_construct(b"DIBogus", b"field"), "debug:DIBogus.field");
}

/// WP-149: a rank over 8 bits is no atom; a location in place is uniqued
/// unless distinct.
#[test]
fn locations_and_their_atoms() {
    assert_eq!(di_atom(5, 3), Some(DiAtom { group: 5, rank: 3 }));
    assert_eq!(di_atom(5, 255).map(|a| a.rank), Some(255));
    assert_eq!(di_atom(5, 256), None);
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let scope = m.debug.reserve().unwrap();
    let loc = DiLocation { line: 3, column: 4, scope, inlined_at: LocId::NONE };
    let atom = DiAtom { group: 1, rank: 1 };
    let a = add_location(&mut m.debug, loc, atom, false).unwrap();
    assert_eq!(add_location(&mut m.debug, loc, atom, false).unwrap(), a);
    assert_ne!(add_location(&mut m.debug, loc, atom, true).unwrap(), a);
}

/// What a `!dbg` or `!llvm.dbg.cu` names must be of the right kind;
/// the module's debug information is checked only when used.
#[test]
fn global_debug_references_and_the_final_check() {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    finish_debug(&mut m, false).unwrap();
    assert!(!noted(&m, DEBUG_INVALID), "no debug information, nothing checked");
    let node = di_node(Some(DiKind::BasicType), b"", []).unwrap();
    let basic = m.debug.add(&node).unwrap();
    assert!(!compile_unit(&mut m, Some(basic)).unwrap());
    assert!(!compile_unit(&mut m, None).unwrap());
    assert_eq!(function_subprogram(&m, Some(basic)), None);
    assert!(!variable_expression(&mut m, SymId::new(0), Some(basic)).unwrap());
    // Listed units but no `Debug Info Version` 3: LLVM drops it all.
    finish_debug(&mut m, true).unwrap();
    assert!(noted(&m, DEBUG_INVALID));
}

/// The four records by name; others are recorded; a `declare` of a
/// `DIArgList` is not made.
#[test]
fn debug_records_by_name_and_their_emission() {
    assert_eq!(DebugRecordKind::of_intrinsic(b"llvm.dbg.assign"), Some(DebugRecordKind::Assign));
    assert_eq!(DebugRecordKind::of_intrinsic(b"llvm.dbg.foo"), None);
    assert_eq!(debug_intrinsic_construct(b"llvm.dbg.foo"), "debug:llvm.dbg.foo");
    assert_eq!(DebugRecordKind::of_record(b"#dbg_label"), Some(DebugRecordKind::Label));
    assert_eq!(DebugRecordKind::of_record(b"#dbg_value"), Some(DebugRecordKind::Value));
    assert_eq!(DebugRecordKind::of_record(b"#dbg_declare_value"), None);
    assert!(unsupported_record_construct(b"#dbg_declare_value").is_some());
    assert_eq!(DebugRecordKind::Label.variable_kind(), DiKind::Label);
    assert_eq!(DebugRecordKind::Declare.variable_kind(), DiKind::LocalVariable);
    let (_, mut b) = fixture();
    let mut record = DebugRecord::new(DebugRecordKind::Declare);
    record.list = true;
    assert!(!emit_debug_record(&mut b, &record, &[Value::NONE], LocId::NONE).unwrap());
}
