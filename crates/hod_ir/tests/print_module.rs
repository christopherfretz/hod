//! `spec/ir.md` §9.3: everything a module owns reaches LLVM as hod means
//! it. One module with every kind of symbol, linkage, visibility,
//! thread-local model, section, alignment, COMDAT, address space, alias,
//! IFUNC, attribute set, parameter attribute, retained symbol,
//! constructor, module flag, linker option and name that needs quoting is
//! built through `hod_ir`, printed for LLVM 22 and LLVM 19, and checked
//! three ways:
//!
//! - `llvm-as` and `opt -passes=verify` accept it;
//! - LLVM's own text for it (`opt -S`) has, for every symbol and every
//!   piece of metadata, the line hod means;
//! - the object `llc` makes of it has every symbol with the binding,
//!   visibility, type, section, size and alignment hod means, and the
//!   sections, COMDAT groups and string contents hod means.

mod print_support;

use hod_ir::{
    AliasDesc, Body, BranchProtection, CallConv, CodeModel, ComdatKind, Dialect, FlagValue, FnAttrs,
    FnStr, FramePointer, FunctionDesc, IFuncDesc, Linkage, Module, ParamAttrs, ParamMem, Part,
    PicLevel, PrintError, Retained, SymId, TargetSpec, TlsModel, TypeId, UnnamedAddr, UwTable,
    VariableDesc, Visibility,
};
use hod_testutil::Scratch;
use print_support::{ElfSymbol, Fx, Llvm, define, int, parse_symbols, printed};

const VOID: TypeId = TypeId::VOID;
const I1: TypeId = TypeId::I1;
const I8: TypeId = TypeId::I8;
const I32: TypeId = TypeId::I32;
const I64: TypeId = TypeId::I64;
const PTR: TypeId = TypeId::PTR;

/// The bytes 1 to 255 in names of sixteen bytes or fewer: a NUL cannot be
/// in a name.
fn byte_names() -> Vec<Vec<u8>> {
    (1..=255u8).collect::<Vec<u8>>().chunks(16).map(<[u8]>::to_vec).collect()
}

/// The function attribute flags a dialect has (§9.2).
fn fn_flags(major: u32) -> u32 {
    let mut flags = 0;
    for (name, flag) in FnAttrs::FLAG_NAMES {
        let since = match name {
            "sanitize_alloc_token" => 22,
            "sanitize_realtime" | "sanitize_realtime_blocking" | "sanitize_type" => 20,
            _ => 19,
        };
        if since <= major {
            flags |= flag;
        }
    }
    flags
}

/// The module, and the bodies of its defined functions. `major` picks the
/// attribute flags; `for_object` leaves out what `llc` cannot put in an
/// AArch64 ELF object (the COMDAT kinds ELF lacks).
#[expect(clippy::too_many_lines, reason = "one long list of symbols")]
fn everything(major: u32, for_object: bool) -> (Module, Vec<Body>) {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let mut bodies = Vec::new();

    // ---- module-level properties ----
    m.props.pic = PicLevel::Big;
    m.props.pie = PicLevel::Small;
    m.props.code_model = Some(CodeModel::Small);
    m.props.large_data_threshold = Some(1 << 33);
    m.props.uwtable = UwTable::Async;
    m.props.frame_pointer = FramePointer::NonLeaf;
    m.props.branch_protection =
        BranchProtection::from_bits(BranchProtection::BTI | BranchProtection::PAC_RET);
    m.props.dwarf_version = Some(5);
    m.props.debug_info_version = Some(3);
    m.set_module_flag(1, b"wchar_size", FlagValue::Int(4)).unwrap();
    m.set_module_flag(7, b"hod.wide", FlagValue::Int(1 << 40)).unwrap();
    m.set_module_flag(5, b"hod.text", FlagValue::Str(b"a \"quoted\" \\ string\n")).unwrap();
    m.set_module_flag(4, b"hod.four", FlagValue::Int(0)).unwrap();
    m.append_module_asm(b"\t.globl\tasm_symbol\nasm_symbol = 42");
    m.append_module_asm(b"\t.ident\t\"a \\\"quoted\\\" line\"\n");
    m.add_linker_option(&[b"-lz"]).unwrap();
    m.add_linker_option(&[b"--defsym", b"a \"b\"=1"]).unwrap();
    m.add_dependent_library(b"m").unwrap();
    m.add_dependent_library(b"with space").unwrap();

    // ---- types ----
    let bytes4 = m.types.array(I8, 4).unwrap();
    let pair = m.types.struct_type(&[I8, I64], false).unwrap();
    let packed = m.types.struct_type(&[I8, I64], true).unwrap();
    let opaque = m.types.opaque_struct().unwrap();
    let void_fn = m.types.function(VOID, &[], false).unwrap();
    let resolver_fn = m.types.function(PTR, &[], false).unwrap();
    let bits = m.types.array(I1, 3).unwrap();

    // ---- constants ----
    let five = int(&mut m, I32, 5);
    let text = m.consts.bytes(&mut m.types, b"abc\0").unwrap();
    let zero32 = m.consts.zero(&m.types, I32).unwrap();
    let one_byte = int(&mut m, I8, 1);
    let big = int(&mut m, I64, -2);
    let pair_value = m.consts.aggregate(&m.types, pair, &[one_byte, big]).unwrap();
    let packed_value = m.consts.aggregate(&m.types, packed, &[one_byte, big]).unwrap();
    let all_bytes: Vec<u8> = (0..=255).collect();
    let all_bytes = m.consts.bytes(&mut m.types, &all_bytes).unwrap();
    let all_bytes_ty = m.consts.ty(all_bytes);
    let truth = int(&mut m, I1, 1);
    let bits_value = m.consts.aggregate(&m.types, bits, &[truth, truth, truth]).unwrap();

    // ---- variables: linkage, visibility, the rest ----
    let var = |m: &mut Module, name: &str, desc: &VariableDesc<'_>| {
        m.add_variable(name.as_bytes(), desc).unwrap_or_else(|error| panic!("{name}: {error}"))
    };
    let base = VariableDesc { init: Some(five), ..VariableDesc::new(I32) };
    let v_ext = var(&mut m, "v_ext", &base);
    let v_const = var(
        &mut m,
        "v_const",
        &VariableDesc { constant: true, init: Some(text), ..VariableDesc::new(bytes4) },
    );
    let v_internal = var(&mut m, "v_internal", &VariableDesc { linkage: Linkage::Internal, ..base });
    var(&mut m, "v_private", &VariableDesc { linkage: Linkage::Private, ..base });
    var(&mut m, "v_weak", &VariableDesc { linkage: Linkage::WeakAny, ..base });
    var(&mut m, "v_weak_odr", &VariableDesc { linkage: Linkage::WeakOdr, ..base });
    var(&mut m, "v_linkonce", &VariableDesc { linkage: Linkage::LinkOnceAny, ..base });
    var(&mut m, "v_linkonce_odr", &VariableDesc { linkage: Linkage::LinkOnceOdr, ..base });
    var(
        &mut m,
        "v_common",
        &VariableDesc { linkage: Linkage::Common, init: Some(zero32), ..VariableDesc::new(I32) },
    );
    var(&mut m, "v_available", &VariableDesc { linkage: Linkage::AvailableExternally, ..base });
    let v_extern_weak = var(
        &mut m,
        "v_extern_weak",
        &VariableDesc { linkage: Linkage::ExternWeak, ..VariableDesc::new(I32) },
    );
    let v_declared = var(&mut m, "v_declared", &VariableDesc::new(I32));
    var(&mut m, "v_hidden", &VariableDesc { visibility: Visibility::Hidden, ..base });
    var(&mut m, "v_protected", &VariableDesc { visibility: Visibility::Protected, ..base });
    var(&mut m, "v_dso_local", &VariableDesc { dso_local: true, ..base });
    var(
        &mut m,
        "v_unnamed",
        &VariableDesc { unnamed_addr: UnnamedAddr::Global, constant: true, ..base },
    );
    var(
        &mut m,
        "v_local_unnamed",
        &VariableDesc { unnamed_addr: UnnamedAddr::Local, constant: true, ..base },
    );
    for (name, tls) in [
        ("v_tls", TlsModel::GeneralDynamic),
        ("v_tls_ld", TlsModel::LocalDynamic),
        ("v_tls_ie", TlsModel::InitialExec),
        ("v_tls_le", TlsModel::LocalExec),
    ] {
        var(&mut m, name, &VariableDesc { tls, ..base });
    }
    var(
        &mut m,
        "v_tls_zero",
        &VariableDesc {
            tls: TlsModel::GeneralDynamic,
            init: Some(zero32),
            ..VariableDesc::new(I32)
        },
    );
    var(&mut m, "v_section", &VariableDesc { section: Some(b"my \"own\" section"), ..base });
    var(&mut m, "v_align", &VariableDesc { align: Some(64), ..base });
    var(&mut m, "v_addr_space", &VariableDesc { addr_space: 1, ..base });
    var(&mut m, "v_pair", &VariableDesc { init: Some(pair_value), ..VariableDesc::new(pair) });
    var(&mut m, "v_packed", &VariableDesc { init: Some(packed_value), ..VariableDesc::new(packed) });
    var(&mut m, "v_opaque", &VariableDesc::new(opaque));
    var(&mut m, "v_zero", &VariableDesc { init: Some(zero32), ..VariableDesc::new(I32) });
    var(
        &mut m,
        "v_bytes",
        &VariableDesc { constant: true, init: Some(all_bytes), ..VariableDesc::new(all_bytes_ty) },
    );
    // LLVM 19 and LLVM 22 align an `i1` differently by default (§9.2).
    var(&mut m, "v_bit", &VariableDesc { init: Some(truth), ..VariableDesc::new(I1) });
    var(&mut m, "v_bits", &VariableDesc { init: Some(bits_value), ..VariableDesc::new(bits) });

    // ---- COMDATs ----
    let group = m.comdat_named(b"group").unwrap();
    let alone = m.comdat_named(b"v_nodeduplicate").unwrap();
    m.set_comdat_kind(alone, ComdatKind::NoDeduplicate);
    var(&mut m, "v_grouped", &VariableDesc { comdat: Some(group), ..base });
    var(&mut m, "v_nodeduplicate", &VariableDesc { comdat: Some(alone), ..base });
    if !for_object {
        for (name, kind) in [
            ("v_exact", ComdatKind::ExactMatch),
            ("v_largest", ComdatKind::Largest),
            ("v_same_size", ComdatKind::SameSize),
        ] {
            let comdat = m.comdat_named(name.as_bytes()).unwrap();
            m.set_comdat_kind(comdat, kind);
            var(&mut m, name, &VariableDesc { comdat: Some(comdat), ..base });
        }
        let quoted = m.comdat_named(b"a comdat").unwrap();
        var(&mut m, "v_quoted_comdat", &VariableDesc { comdat: Some(quoted), ..base });
    }

    // ---- names: quoted, escaped, and none at all ----
    for name in ["a b", "quote\"d", "back\\slash", "1digit", "$dollar-dash.dot_", "\u{fc}n\u{ef}"] {
        var(&mut m, name, &base);
    }
    for name in byte_names() {
        m.add_variable(&name, &VariableDesc { linkage: Linkage::Internal, ..base }).unwrap();
    }
    let unnamed = m.anonymous_symbol().unwrap();
    m.set_variable(unnamed, &VariableDesc { linkage: Linkage::Private, ..base }).unwrap();
    let _gap = m.anonymous_symbol().unwrap();
    m.set_variable(_gap, &VariableDesc { linkage: Linkage::Internal, ..base }).unwrap();

    // ---- variables whose initializers name other symbols ----
    let refs = [
        m.consts.addr(v_ext, 0).unwrap(),
        m.consts.addr(v_const, 2).unwrap(),
        m.consts.addr(v_extern_weak, 0).unwrap(),
        m.consts.addr(v_declared, -4).unwrap(),
        m.consts.addr(unnamed, 0).unwrap(),
    ];
    let refs_ty = m.types.array(PTR, refs.len() as u64).unwrap();
    let refs = m.consts.aggregate(&m.types, refs_ty, &refs).unwrap();
    var(&mut m, "v_refs", &VariableDesc { init: Some(refs), ..VariableDesc::new(refs_ty) });

    // ---- functions ----
    let function = |m: &mut Module, name: &str, desc: &FunctionDesc<'_>| {
        m.add_function(name.as_bytes(), desc).unwrap_or_else(|error| panic!("{name}: {error}"))
    };
    let defined = FunctionDesc { has_body: true, ..FunctionDesc::new(void_fn) };
    let mut define_void = |m: &mut Module, name: &str, desc: &FunctionDesc<'_>| {
        let f = function(m, name, desc);
        let mut x = Fx::new(m, f);
        x.ret_void();
        bodies.push(x.done());
        f
    };
    let f_plain = define_void(&mut m, "f_plain", &defined);
    let f_internal =
        define_void(&mut m, "f_internal", &FunctionDesc { linkage: Linkage::Internal, ..defined });
    define_void(&mut m, "f_private", &FunctionDesc { linkage: Linkage::Private, ..defined });
    define_void(&mut m, "f_weak_odr", &FunctionDesc { linkage: Linkage::WeakOdr, ..defined });
    define_void(&mut m, "f_linkonce_odr", &FunctionDesc { linkage: Linkage::LinkOnceOdr, ..defined });
    define_void(
        &mut m,
        "f_available",
        &FunctionDesc { linkage: Linkage::AvailableExternally, ..defined },
    );
    define_void(
        &mut m,
        "f_hidden",
        &FunctionDesc { visibility: Visibility::Hidden, unnamed_addr: UnnamedAddr::Global, ..defined },
    );
    define_void(
        &mut m,
        "f_protected",
        &FunctionDesc {
            visibility: Visibility::Protected,
            dso_local: true,
            unnamed_addr: UnnamedAddr::Local,
            ..defined
        },
    );
    define_void(
        &mut m,
        "f_placed",
        &FunctionDesc {
            section: Some(b".text.placed"),
            align: Some(64),
            comdat: Some(group),
            cc: CallConv::FAST,
            ..defined
        },
    );
    let personality = function(
        &mut m,
        "__gxx_personality_v0",
        &FunctionDesc::new(void_fn),
    );
    define_void(&mut m, "f_personality", &FunctionDesc { personality: Some(personality), ..defined });
    function(&mut m, "f_extern_weak", &FunctionDesc { linkage: Linkage::ExternWeak, ..defined_no_body(void_fn) });
    function(&mut m, "f_addr_space", &FunctionDesc { addr_space: 1, ..defined_no_body(void_fn) });
    function(&mut m, "f_cold", &FunctionDesc { cc: CallConv::COLD, ..defined_no_body(void_fn) });
    function(
        &mut m,
        "f_numbered_cc",
        &FunctionDesc { cc: CallConv::from_llvm(1023).unwrap(), ..defined_no_body(void_fn) },
    );

    // Attribute sets: every flag the dialect has, every string key.
    let mut attrs = FnAttrs::new();
    attrs.set(fn_flags(major)).unwrap();
    attrs.set_uwtable(UwTable::Sync);
    attrs.set_stack_align(32).unwrap();
    let all_flags = m.intern_fn_attrs(&attrs).unwrap();
    function(&mut m, "f_flags", &FunctionDesc { attrs: all_flags, ..defined_no_body(void_fn) });
    let mut attrs = FnAttrs::new();
    attrs.set_uwtable(UwTable::Async);
    for key in FnStr::ALL {
        let value: &[u8] = match key {
            FnStr::TargetCpu | FnStr::TuneCpu => b"generic",
            FnStr::TargetFeatures => b"+neon,+fp-armv8",
            FnStr::FramePointer => b"non-leaf",
            FnStr::PatchableFunctionEntry | FnStr::PatchableFunctionPrefix => b"2",
            FnStr::FunctionInstrument => b"xray-always",
            FnStr::BranchTargetEnforcement
            | FnStr::BranchProtectionPauthLr
            | FnStr::GuardedControlStack => b"true",
            FnStr::SignReturnAddress => b"non-leaf",
            FnStr::SignReturnAddressKey => b"a_key",
            FnStr::StackProbeSize => b"4096",
            FnStr::ProbeStack => b"inline-asm",
            FnStr::SplitStack | FnStr::NoJumpTables | FnStr::NoBuiltins => b"",
            FnStr::ZeroCallUsedRegs => b"used-gpr",
            FnStr::StackProtectorBufferSize => b"8",
            FnStr::MinLegalVectorWidth => b"128",
            FnStr::NoBuiltin => b"memcpy",
        };
        attrs.set_string(key, value).unwrap();
    }
    let all_strings = m.intern_fn_attrs(&attrs).unwrap();
    define_void(&mut m, "f_strings", &FunctionDesc { attrs: all_strings, ..defined });
    // The same set twice is one group.
    function(&mut m, "f_strings_again", &FunctionDesc { attrs: all_strings, ..defined_no_body(void_fn) });

    // Parameter attributes on a declaration.
    let flag = |flags| ParamAttrs::NONE.with(flags).unwrap();
    let memory = |mem| ParamAttrs::NONE.with_mem(mem, pair).unwrap().with_align(16).unwrap();
    let params = [PTR, I8, I8, I64, PTR, PTR, PTR, PTR, PTR, PTR, PTR];
    let param_attrs = [
        memory(ParamMem::Sret),
        flag(ParamAttrs::ZEROEXT),
        flag(ParamAttrs::SIGNEXT),
        flag(ParamAttrs::INREG),
        flag(ParamAttrs::NEST),
        flag(ParamAttrs::SWIFT_SELF),
        flag(ParamAttrs::SWIFT_ERROR),
        flag(ParamAttrs::SWIFT_ASYNC),
        memory(ParamMem::ByVal),
        memory(ParamMem::ByRef),
        ParamAttrs::NONE.with_align(8).unwrap().with_stack_align(16).unwrap(),
    ];
    let attributed = m.types.function(VOID, &params, true).unwrap();
    function(
        &mut m,
        "f_params",
        &FunctionDesc { param_attrs: &param_attrs, ..FunctionDesc::new(attributed) },
    );
    let memory_only = m.types.function(VOID, &[PTR], false).unwrap();
    for (name, mem) in [("f_inalloca", ParamMem::InAlloca), ("f_preallocated", ParamMem::Preallocated)] {
        let attrs = [ParamAttrs::NONE.with_mem(mem, pair).unwrap()];
        function(&mut m, name, &FunctionDesc { param_attrs: &attrs, ..FunctionDesc::new(memory_only) });
    }
    let narrow = m.types.function(I8, &[], false).unwrap();
    function(
        &mut m,
        "f_ret_attr",
        &FunctionDesc { ret_attrs: flag(ParamAttrs::SIGNEXT), ..FunctionDesc::new(narrow) },
    );

    // ---- aliases and an IFUNC ----
    let alias = |m: &mut Module, name: &str, desc: &AliasDesc| {
        m.add_alias(name.as_bytes(), desc).unwrap_or_else(|error| panic!("{name}: {error}"))
    };
    let to_ext = m.consts.addr(v_ext, 0).unwrap();
    let into_const = m.consts.addr(v_const, 2).unwrap();
    let to_plain = m.consts.addr(f_plain, 0).unwrap();
    alias(&mut m, "a_var", &AliasDesc::new(I32, to_ext));
    alias(&mut m, "a_offset", &AliasDesc { unnamed_addr: UnnamedAddr::Global, ..AliasDesc::new(I8, into_const) });
    alias(
        &mut m,
        "a_function",
        &AliasDesc {
            linkage: Linkage::WeakAny,
            visibility: Visibility::Hidden,
            ..AliasDesc::new(void_fn, to_plain)
        },
    );
    alias(&mut m, "a_internal", &AliasDesc { linkage: Linkage::Internal, ..AliasDesc::new(void_fn, to_plain) });
    let resolver = function(
        &mut m,
        "resolver",
        &FunctionDesc { linkage: Linkage::Internal, has_body: true, ..FunctionDesc::new(resolver_fn) },
    );
    let mut x = Fx::new(&mut m, resolver);
    let chosen = x.sym(f_plain);
    x.ret(chosen);
    bodies.push(x.done());
    m.add_ifunc(b"i_function", &IFuncDesc::new(void_fn, resolver)).unwrap();
    m.add_ifunc(
        b"i_hidden",
        &IFuncDesc { visibility: Visibility::Hidden, ..IFuncDesc::new(void_fn, resolver) },
    )
    .unwrap();

    // ---- retained symbols, constructors and destructors ----
    m.retain(v_internal, Retained::Used).unwrap();
    m.retain(f_internal, Retained::Used).unwrap();
    m.retain(v_const, Retained::CompilerUsed).unwrap();
    m.add_ctor(65535, f_plain, None);
    m.add_ctor(101, f_internal, Some(v_ext));
    m.add_dtor(0, f_plain, None);

    m.verify_symbols().unwrap();
    let _: SymId = v_declared;
    (m, bodies)
}

/// A declaration of type `ty`.
fn defined_no_body(ty: TypeId) -> FunctionDesc<'static> {
    FunctionDesc::new(ty)
}

#[test]
fn explore() {
    let Ok(dir) = std::env::var("HOD_WP006_DUMP") else { return };
    for major in [22, 19] {
        let (m, bodies) = everything(major, true);
        let text = printed(&m, Dialect::new(major).unwrap(), &bodies);
        std::fs::write(format!("{dir}/module{major}.ll"), &text).unwrap();
        let (m, bodies) = everything(major, false);
        let text = printed(&m, Dialect::new(major).unwrap(), &bodies);
        std::fs::write(format!("{dir}/module{major}-text.ll"), &text).unwrap();
    }
    let _ = (Scratch::new("x"), Part::Whole, PrintError::Write);
    let _: Option<(Llvm, ElfSymbol)> = None;
    let _ = parse_symbols("");
    let _ = define;
}
