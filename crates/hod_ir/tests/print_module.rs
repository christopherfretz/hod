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
    AliasDesc, BlockId, Body, BranchProtection, CallConv, CodeModel, ComdatId, ComdatKind, Dialect,
    FlagValue, FnAttrs, FnStr, FramePointer, FunctionDesc, IFuncDesc, IoSink, Linkage, Module,
    ParamAttrs, ParamMem, Part, PicLevel, PrintError, Retained, SymId, TargetSpec, TlsModel,
    TypeId, UnnamedAddr, UwTable, VariableDesc, Visibility,
};
use hod_testutil::Scratch;
use print_support::{Fx, Llvm, int, parse_symbols, printed};

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

/// The function attribute flags a dialect has (§9.2), but
/// `sanitize_realtime_blocking`: LLVM refuses it with `sanitize_realtime`
/// (and so does `FnAttrs::set`, §5.8).
fn fn_flags(major: u32) -> u32 {
    let mut flags = 0;
    for (name, flag) in FnAttrs::FLAG_NAMES {
        let since = match name {
            "sanitize_realtime_blocking" => continue,
            "sanitize_alloc_token" => 22,
            "sanitize_realtime" | "sanitize_type" => 20,
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
    // Behavior 5 (append) takes a node in LLVM, not a string (§9.8).
    m.set_module_flag(1, b"hod.text", FlagValue::Str(b"a \"quoted\" \\ string\n")).unwrap();
    m.set_module_flag(4, b"hod.four", FlagValue::Int(0)).unwrap();
    m.append_module_asm(b"\t.globl\tasm_symbol\nasm_symbol = 42");
    m.append_module_asm(b"\t.ident\t\"a \\\"quoted\\\" line\"\n");
    if !for_object {
        // ELF `llc` wants each option a key and a value (§9.8).
        m.add_linker_option(&[b"-lz"]).unwrap();
    }
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
    let v_internal =
        var(&mut m, "v_internal", &VariableDesc { linkage: Linkage::Internal, ..base });
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
    var(
        &mut m,
        "v_packed",
        &VariableDesc { init: Some(packed_value), ..VariableDesc::new(packed) },
    );
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
    let gap = m.anonymous_symbol().unwrap();
    m.set_variable(gap, &VariableDesc { linkage: Linkage::Internal, ..base }).unwrap();

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
    define_void(
        &mut m,
        "f_linkonce_odr",
        &FunctionDesc { linkage: Linkage::LinkOnceOdr, ..defined },
    );
    define_void(
        &mut m,
        "f_available",
        &FunctionDesc { linkage: Linkage::AvailableExternally, ..defined },
    );
    define_void(
        &mut m,
        "f_hidden",
        &FunctionDesc {
            visibility: Visibility::Hidden,
            unnamed_addr: UnnamedAddr::Global,
            ..defined
        },
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
    let personality = function(&mut m, "__gxx_personality_v0", &FunctionDesc::new(void_fn));
    define_void(
        &mut m,
        "f_personality",
        &FunctionDesc { personality: Some(personality), ..defined },
    );
    function(
        &mut m,
        "f_extern_weak",
        &FunctionDesc { linkage: Linkage::ExternWeak, ..FunctionDesc::new(void_fn) },
    );
    function(&mut m, "f_addr_space", &FunctionDesc { addr_space: 1, ..FunctionDesc::new(void_fn) });
    function(&mut m, "f_cold", &FunctionDesc { cc: CallConv::COLD, ..FunctionDesc::new(void_fn) });
    function(
        &mut m,
        "f_numbered_cc",
        &FunctionDesc { cc: CallConv::from_llvm(1023).unwrap(), ..FunctionDesc::new(void_fn) },
    );

    // Attribute sets: every flag the dialect has, every string key.
    let mut attrs = FnAttrs::new();
    attrs.set(fn_flags(major)).unwrap();
    attrs.set_uwtable(UwTable::Sync);
    attrs.set_stack_align(32).unwrap();
    let all_flags = m.intern_fn_attrs(&attrs).unwrap();
    function(&mut m, "f_flags", &FunctionDesc { attrs: all_flags, ..FunctionDesc::new(void_fn) });
    let mut attrs = FnAttrs::new();
    attrs.set_uwtable(UwTable::Async);
    for key in FnStr::ALL {
        let value: &[u8] = match key {
            FnStr::TargetCpu | FnStr::TuneCpu => b"generic",
            FnStr::TargetFeatures => b"+neon,+fp-armv8",
            FnStr::FramePointer | FnStr::SignReturnAddress => b"non-leaf",
            FnStr::PatchableFunctionEntry | FnStr::PatchableFunctionPrefix => b"2",
            FnStr::FunctionInstrument => b"xray-always",
            FnStr::BranchTargetEnforcement
            | FnStr::BranchProtectionPauthLr
            | FnStr::GuardedControlStack => b"true",
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
    function(
        &mut m,
        "f_strings_again",
        &FunctionDesc { attrs: all_strings, ..FunctionDesc::new(void_fn) },
    );

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
    for (name, mem) in
        [("f_inalloca", ParamMem::InAlloca), ("f_preallocated", ParamMem::Preallocated)]
    {
        let attrs = [ParamAttrs::NONE.with_mem(mem, pair).unwrap()];
        function(
            &mut m,
            name,
            &FunctionDesc { param_attrs: &attrs, ..FunctionDesc::new(memory_only) },
        );
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
    alias(
        &mut m,
        "a_offset",
        &AliasDesc { unnamed_addr: UnnamedAddr::Global, ..AliasDesc::new(I8, into_const) },
    );
    alias(
        &mut m,
        "a_function",
        &AliasDesc {
            linkage: Linkage::WeakAny,
            visibility: Visibility::Hidden,
            ..AliasDesc::new(void_fn, to_plain)
        },
    );
    alias(
        &mut m,
        "a_internal",
        &AliasDesc { linkage: Linkage::Internal, ..AliasDesc::new(void_fn, to_plain) },
    );
    let resolver = function(
        &mut m,
        "resolver",
        &FunctionDesc {
            linkage: Linkage::Internal,
            has_body: true,
            ..FunctionDesc::new(resolver_fn)
        },
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
    (m, bodies)
}

/// The lines LLVM's own text (`opt -S`) has for the module of `major`:
/// one per symbol, COMDAT, `module asm` line and metadata node. LLVM
/// renumbers the anonymous symbols and quotes `$`-names its own way.
#[expect(clippy::too_many_lines, reason = "one line per symbol")]
fn expected_lines(major: u32) -> Vec<String> {
    let mut lines: Vec<String> = [
        r#"target triple = "aarch64-unknown-linux-gnu""#,
        r#"module asm "\09.globl\09asm_symbol""#,
        r#"module asm "asm_symbol = 42""#,
        r#"module asm "\09.ident\09\22a \\\22quoted\\\22 line\22""#,
        "%hod.opaque.0 = type opaque",
        "$group = comdat any",
        "$v_nodeduplicate = comdat nodeduplicate",
        "$v_exact = comdat exactmatch",
        "$v_largest = comdat largest",
        "$v_same_size = comdat samesize",
        r#"$"a comdat" = comdat any"#,
        "@v_ext = global i32 5",
        r#"@v_const = constant [4 x i8] c"abc\00""#,
        "@v_internal = internal global i32 5",
        "@v_private = private global i32 5",
        "@v_weak = weak global i32 5",
        "@v_weak_odr = weak_odr global i32 5",
        "@v_linkonce = linkonce global i32 5",
        "@v_linkonce_odr = linkonce_odr global i32 5",
        "@v_common = common global i32 0",
        "@v_available = available_externally global i32 5",
        "@v_extern_weak = extern_weak global i32",
        "@v_declared = external global i32",
        "@v_hidden = hidden global i32 5",
        "@v_protected = protected global i32 5",
        "@v_dso_local = dso_local global i32 5",
        "@v_unnamed = unnamed_addr constant i32 5",
        "@v_local_unnamed = local_unnamed_addr constant i32 5",
        "@v_tls = thread_local global i32 5",
        "@v_tls_ld = thread_local(localdynamic) global i32 5",
        "@v_tls_ie = thread_local(initialexec) global i32 5",
        "@v_tls_le = thread_local(localexec) global i32 5",
        "@v_tls_zero = thread_local global i32 0",
        r#"@v_section = global i32 5, section "my \22own\22 section""#,
        "@v_align = global i32 5, align 64",
        "@v_addr_space = addrspace(1) global i32 5",
        "@v_pair = global { i8, i64 } { i8 1, i64 -2 }",
        "@v_packed = global <{ i8, i64 }> <{ i8 1, i64 -2 }>",
        "@v_opaque = external global %hod.opaque.0",
        "@v_zero = global i32 0",
        "@v_grouped = global i32 5, comdat($group)",
        "@v_nodeduplicate = global i32 5, comdat",
        "@v_exact = global i32 5, comdat",
        "@v_largest = global i32 5, comdat",
        "@v_same_size = global i32 5, comdat",
        r#"@v_quoted_comdat = global i32 5, comdat($"a comdat")"#,
        r#"@"a b" = global i32 5"#,
        r#"@"quote\22d" = global i32 5"#,
        r#"@"back\\slash" = global i32 5"#,
        r#"@"1digit" = global i32 5"#,
        r#"@"$dollar-dash.dot_" = global i32 5"#,
        r#"@"\C3\BCn\C3\AF" = global i32 5"#,
        r#"@"\01\02\03\04\05\06\07\08\09\0A\0B\0C\0D\0E\0F\10" = internal global i32 5"#,
        r#"@"!\22#$%&'()*+,-./0" = internal global i32 5"#,
        r#"@"QRSTUVWXYZ[\\]^_`" = internal global i32 5"#,
        "@ABCDEFGHIJKLMNOP = internal global i32 5",
        r#"@"\F1\F2\F3\F4\F5\F6\F7\F8\F9\FA\FB\FC\FD\FE\FF" = internal global i32 5"#,
        "@0 = private global i32 5",
        "@1 = internal global i32 5",
        "@v_refs = global [5 x ptr] [ptr @v_ext, ptr getelementptr (i8, ptr @v_const, i64 2), \
         ptr @v_extern_weak, ptr getelementptr (i8, ptr @v_declared, i64 -4), ptr @0]",
        r#"@llvm.used = appending global [2 x ptr] [ptr @v_internal, ptr @f_internal], section "llvm.metadata""#,
        r#"@llvm.compiler.used = appending global [1 x ptr] [ptr @v_const], section "llvm.metadata""#,
        "@llvm.global_ctors = appending global [2 x { i32, ptr, ptr }] [{ i32, ptr, ptr } \
         { i32 65535, ptr @f_plain, ptr null }, { i32, ptr, ptr } { i32 101, ptr @f_internal, ptr @v_ext }]",
        "@llvm.global_dtors = appending global [1 x { i32, ptr, ptr }] [{ i32, ptr, ptr } \
         { i32 0, ptr @f_plain, ptr null }]",
        "@a_var = alias i32, ptr @v_ext",
        "@a_offset = unnamed_addr alias i8, getelementptr (i8, ptr @v_const, i64 2)",
        "@a_function = weak hidden alias void (), ptr @f_plain",
        "@a_internal = internal alias void (), ptr @f_plain",
        "@i_function = ifunc void (), ptr @resolver",
        "@i_hidden = hidden ifunc void (), ptr @resolver",
        "define void @f_plain() {",
        "define internal void @f_internal() {",
        "define private void @f_private() {",
        "define weak_odr void @f_weak_odr() {",
        "define linkonce_odr void @f_linkonce_odr() {",
        "define available_externally void @f_available() {",
        "define hidden void @f_hidden() unnamed_addr {",
        "define protected void @f_protected() local_unnamed_addr {",
        r#"define fastcc void @f_placed() section ".text.placed" comdat($group) align 64 {"#,
        "declare void @__gxx_personality_v0()",
        "define void @f_personality() personality ptr @__gxx_personality_v0 {",
        "declare extern_weak void @f_extern_weak()",
        "declare void @f_addr_space() addrspace(1)",
        "declare coldcc void @f_cold()",
        "declare cc1023 void @f_numbered_cc()",
        "declare void @f_flags() #0",
        "define void @f_strings() #1 {",
        "declare void @f_strings_again() #1",
        "declare void @f_params(ptr sret({ i8, i64 }) align 16, i8 zeroext, i8 signext, i64 inreg, \
         ptr nest, ptr swiftself, ptr swifterror, ptr swiftasync, ptr byval({ i8, i64 }) align 16, \
         ptr byref({ i8, i64 }) align 16, ptr align 8 alignstack(16), ...)",
        "declare void @f_inalloca(ptr inalloca({ i8, i64 }))",
        "declare void @f_preallocated(ptr preallocated({ i8, i64 }))",
        "declare signext i8 @f_ret_attr()",
        "define internal ptr @resolver() {",
        r#"!0 = !{i32 8, !"PIC Level", i32 2}"#,
        r#"!1 = !{i32 7, !"PIE Level", i32 1}"#,
        r#"!2 = !{i32 1, !"Code Model", i32 1}"#,
        r#"!3 = !{i32 1, !"Large Data Threshold", i64 8589934592}"#,
        r#"!4 = !{i32 7, !"uwtable", i32 2}"#,
        r#"!5 = !{i32 7, !"frame-pointer", i32 1}"#,
        r#"!6 = !{i32 7, !"Dwarf Version", i32 5}"#,
        r#"!7 = !{i32 2, !"Debug Info Version", i32 3}"#,
        r#"!10 = !{i32 1, !"wchar_size", i32 4}"#,
        r#"!11 = !{i32 7, !"hod.wide", i64 1099511627776}"#,
        r#"!12 = !{i32 1, !"hod.text", !"a \22quoted\22 \\ string\0A"}"#,
        r#"!13 = !{i32 4, !"hod.four", i32 0}"#,
        "!llvm.module.flags = !{!0, !1, !2, !3, !4, !5, !6, !7, !8, !9, !10, !11, !12, !13}",
        r#"!14 = !{!"-lz"}"#,
        r#"!15 = !{!"--defsym", !"a \22b\22=1"}"#,
        "!llvm.linker.options = !{!14, !15}",
        r#"!16 = !{!"m"}"#,
        r#"!17 = !{!"with space"}"#,
        "!llvm.dependent-libraries = !{!16, !17}",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    // §9.2: LLVM 19 is given the alignment of an `i1` global, and its
    // branch-protection flags are 1 where LLVM 22's are 2.
    let (bit_align, protection) = if major < 20 { (", align 4", 1) } else { ("", 2) };
    lines.push(format!("@v_bit = global i1 true{bit_align}"));
    lines.push(format!("@v_bits = global [3 x i1] [i1 true, i1 true, i1 true]{bit_align}"));
    lines.push(format!(r#"!8 = !{{i32 8, !"branch-target-enforcement", i32 {protection}}}"#));
    lines.push(format!(r#"!9 = !{{i32 8, !"sign-return-address", i32 {protection}}}"#));
    lines
}

/// `spec/ir.md` §9.3: LLVM 22 and LLVM 19 accept the module and verify it,
/// and their own text for it has the line hod means for every symbol,
/// COMDAT, attribute group and metadata node.
#[test]
fn module_text_is_accepted_and_means_what_hod_means() {
    for major in hod_testutil::LLVM_MAJORS {
        let Some(llvm) = Llvm::new(major) else { continue };
        let (m, bodies) = everything(major, false);
        let text = printed(&m, llvm.dialect(), &bodies);
        llvm.accept(&text, "the module");
        let reprint = llvm.reprint(&text, "the module");
        let lines: Vec<&str> = reprint.lines().collect();
        for line in expected_lines(major) {
            assert!(lines.contains(&line.as_str()), "LLVM {major}: no line\n{line}\nin\n{reprint}");
        }
        // The attribute groups: every flag the dialect has, every string.
        let group = |number: u32| {
            let head = format!("attributes #{number} = {{ ");
            lines.iter().find_map(|line| line.strip_prefix(head.as_str())).unwrap_or_else(|| {
                panic!("LLVM {major}: no attribute group #{number} in\n{reprint}")
            })
        };
        let flags = group(0);
        for (name, flag) in FnAttrs::FLAG_NAMES {
            let words: Vec<&str> = flags.split(' ').collect();
            assert_eq!(
                words.contains(&name),
                fn_flags(major) & flag != 0,
                "LLVM {major}: `{name}` in `{flags}`"
            );
        }
        assert!(flags.contains("uwtable(sync)") && flags.contains("alignstack=32"), "{flags}");
        let strings = group(1);
        for key in FnStr::ALL {
            let name = key.name();
            assert!(
                strings.contains(&format!("\"{name}")),
                "LLVM {major}: `{name}` in `{strings}`"
            );
        }
        assert!(strings.starts_with("uwtable "), "{strings}");
        eprintln!("print_module: LLVM {major}: {} lines checked", expected_lines(major).len());
    }
}

/// One symbol of the object: name, type, binding, visibility, section,
/// size.
type Expected = (&'static str, &'static str, &'static str, &'static str, &'static str, u64);

/// What `llvm-readelf` shows for the named symbols of the module's object.
/// `llvm-readelf` cuts a section name at its first space.
const SYMBOLS: [Expected; 48] = [
    ("asm_symbol", "NOTYPE", "GLOBAL", "DEFAULT", "ABS", 0),
    ("f_plain", "FUNC", "GLOBAL", "DEFAULT", ".text", 4),
    ("f_internal", "FUNC", "LOCAL", "DEFAULT", ".text.f_internal", 4),
    ("f_weak_odr", "FUNC", "WEAK", "DEFAULT", ".text", 4),
    ("f_linkonce_odr", "FUNC", "WEAK", "DEFAULT", ".text", 4),
    ("f_hidden", "FUNC", "GLOBAL", "HIDDEN", ".text", 4),
    ("f_protected", "FUNC", "GLOBAL", "PROTECTED", ".text", 4),
    ("f_placed", "FUNC", "GLOBAL", "DEFAULT", ".text.placed", 4),
    ("f_personality", "FUNC", "GLOBAL", "DEFAULT", ".text", 4),
    ("f_extern_weak", "NOTYPE", "WEAK", "DEFAULT", "UND", 0),
    ("resolver", "FUNC", "LOCAL", "DEFAULT", ".text", 12),
    ("v_ext", "OBJECT", "GLOBAL", "DEFAULT", ".data", 4),
    ("v_const", "OBJECT", "GLOBAL", "DEFAULT", ".rodata", 4),
    ("v_internal", "OBJECT", "LOCAL", "DEFAULT", ".data.v_internal", 4),
    ("v_weak", "OBJECT", "WEAK", "DEFAULT", ".data", 4),
    ("v_weak_odr", "OBJECT", "WEAK", "DEFAULT", ".data", 4),
    ("v_linkonce", "OBJECT", "WEAK", "DEFAULT", ".data", 4),
    ("v_linkonce_odr", "OBJECT", "WEAK", "DEFAULT", ".data", 4),
    ("v_common", "OBJECT", "GLOBAL", "DEFAULT", "COM", 4),
    ("v_extern_weak", "NOTYPE", "WEAK", "DEFAULT", "UND", 0),
    ("v_declared", "NOTYPE", "GLOBAL", "DEFAULT", "UND", 0),
    ("v_hidden", "OBJECT", "GLOBAL", "HIDDEN", ".data", 4),
    ("v_protected", "OBJECT", "GLOBAL", "PROTECTED", ".data", 4),
    ("v_dso_local", "OBJECT", "GLOBAL", "DEFAULT", ".data", 4),
    ("v_unnamed", "OBJECT", "GLOBAL", "DEFAULT", ".rodata.cst4", 4),
    ("v_local_unnamed", "OBJECT", "GLOBAL", "DEFAULT", ".rodata", 4),
    ("v_tls", "TLS", "GLOBAL", "DEFAULT", ".tdata", 4),
    ("v_tls_ld", "TLS", "GLOBAL", "DEFAULT", ".tdata", 4),
    ("v_tls_ie", "TLS", "GLOBAL", "DEFAULT", ".tdata", 4),
    ("v_tls_le", "TLS", "GLOBAL", "DEFAULT", ".tdata", 4),
    ("v_tls_zero", "TLS", "GLOBAL", "DEFAULT", ".tbss", 4),
    ("v_section", "OBJECT", "GLOBAL", "DEFAULT", "my", 4),
    ("v_addr_space", "OBJECT", "GLOBAL", "DEFAULT", ".data", 4),
    ("v_pair", "OBJECT", "GLOBAL", "DEFAULT", ".data", 16),
    ("v_packed", "OBJECT", "GLOBAL", "DEFAULT", ".data", 9),
    ("v_zero", "OBJECT", "GLOBAL", "DEFAULT", ".bss", 4),
    ("v_bytes", "OBJECT", "GLOBAL", "DEFAULT", ".rodata", 256),
    ("v_bit", "OBJECT", "GLOBAL", "DEFAULT", ".data", 1),
    ("v_bits", "OBJECT", "GLOBAL", "DEFAULT", ".data", 3),
    ("v_grouped", "OBJECT", "GLOBAL", "DEFAULT", ".data.v_grouped", 4),
    ("v_nodeduplicate", "OBJECT", "GLOBAL", "DEFAULT", ".data.v_nodeduplicate", 4),
    ("a b", "OBJECT", "GLOBAL", "DEFAULT", ".data", 4),
    ("v_refs", "OBJECT", "GLOBAL", "DEFAULT", ".data", 40),
    ("a_var", "OBJECT", "GLOBAL", "DEFAULT", ".data", 4),
    ("a_function", "FUNC", "WEAK", "HIDDEN", ".text", 4),
    ("a_internal", "FUNC", "LOCAL", "DEFAULT", ".text", 4),
    ("i_function", "IFUNC", "GLOBAL", "DEFAULT", ".text", 12),
    ("i_hidden", "IFUNC", "GLOBAL", "HIDDEN", ".text", 12),
];

/// `spec/ir.md` §9.3: the object `llc` 22 and 19 make of the module has
/// every symbol with the type, binding, visibility, section and size hod
/// means; aliases at their aliasee's address; the COMDAT groups, linker
/// options and dependent libraries hod means; and the 256 bytes of
/// `v_bytes` in `.rodata`.
#[test]
fn module_object_has_what_hod_means() {
    let scratch = Scratch::new("print-module");
    for major in hod_testutil::LLVM_MAJORS {
        let Some(llvm) = Llvm::new(major) else { continue };
        let (m, bodies) = everything(major, true);
        let text = printed(&m, llvm.dialect(), &bodies);
        let object = llvm.object(&scratch, &format!("module-{major}"), &text, &["-O0"]);
        let (sections, symbols) = parse_symbols(&llvm.readelf(&object, &["-S", "-s", "--wide"]));
        let find = |name: &str| {
            symbols
                .iter()
                .find(|symbol| symbol.name == name)
                .unwrap_or_else(|| panic!("LLVM {major}: no symbol `{name}`"))
        };
        for (name, kind, bind, visibility, section, size) in SYMBOLS {
            let symbol = find(name);
            let seen =
                (&*symbol.kind, &*symbol.bind, &*symbol.visibility, &*symbol.section, symbol.size);
            assert_eq!(seen, (kind, bind, visibility, section, size), "LLVM {major}: `{name}`");
        }
        // Names that need quoting reach the object as they are.
        for name in ["quote\"d", "back\\slash", "1digit", "$dollar-dash.dot_", "\u{fc}n\u{ef}"] {
            assert_eq!(find(name).section, ".data", "LLVM {major}: `{name}`");
        }
        let locals = symbols
            .iter()
            .filter(|symbol| symbol.kind == "OBJECT" && symbol.bind == "LOCAL" && symbol.size == 4)
            .count();
        // The sixteen byte names, `v_internal` and the internal anonymous
        // symbol: private symbols do not reach the symbol table.
        assert_eq!(locals, byte_names().len() + 2, "LLVM {major}");
        for name in ["v_private", "f_private"] {
            assert!(symbols.iter().all(|symbol| symbol.name != name), "LLVM {major}: `{name}`");
        }
        assert_eq!(find("a_var").value, find("v_ext").value, "LLVM {major}");
        assert_eq!(find("a_offset").value, find("v_const").value + 2, "LLVM {major}");
        assert_eq!(find("a_function").value, find("f_plain").value, "LLVM {major}");
        assert_eq!(find("i_function").value, find("resolver").value, "LLVM {major}");
        assert_eq!(find("v_align").value % 64, 0, "LLVM {major}");
        assert!(sections.iter().any(|name| name == "my"), "LLVM {major}: {sections:?}");

        let groups = llvm.readelf(&object, &["-g"]);
        for wanted in [
            "COMDAT group section",
            "[group] contains 2 sections",
            ".text.placed",
            ".data.v_grouped",
        ] {
            assert!(groups.contains(wanted), "LLVM {major}: `{wanted}` in\n{groups}");
        }
        assert!(
            groups.contains("[v_nodeduplicate] contains 1 sections"),
            "LLVM {major}:\n{groups}"
        );

        let rodata = llvm.section(&object, ".rodata");
        let at = usize::try_from(find("v_bytes").value).unwrap();
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(rodata.get(at..at + 256), Some(&all[..]), "LLVM {major}: v_bytes");
        assert_eq!(
            llvm.section(&object, ".linker-options"),
            b"--defsym\0a \"b\"=1\0",
            "LLVM {major}"
        );
        assert_eq!(llvm.section(&object, ".deplibs"), b"m\0with space\0", "LLVM {major}");
        eprintln!("print_module: LLVM {major}: {} symbols checked in the object", SYMBOLS.len());
    }
}

/// A small AArch64 module.
fn small() -> Module {
    Module::new(TargetSpec::aarch64_linux_gnu())
}

/// The construct `part` of `m` is refused for, in `major`'s dialect.
fn refusal(m: &Module, major: u32, part: Part, bodies: &[&Body]) -> String {
    let mut text = String::new();
    let dialect = Dialect::new(major).unwrap();
    match m.print_part(&mut text, dialect, part, bodies.iter().copied()) {
        Err(PrintError::Unsupported { construct, .. }) => construct,
        other => panic!("LLVM {major}: {other:?}, printed\n{text}"),
    }
}

/// Whether `m` prints in `major`'s dialect.
fn prints(m: &Module, major: u32, bodies: &[&Body]) -> bool {
    m.print(&mut String::new(), Dialect::new(major).unwrap(), bodies.iter().copied()).is_ok()
}

/// A function `name` whose entry block branches to a second block that
/// returns, and its body.
fn two_blocks(m: &mut Module, name: &str) -> (SymId, Body) {
    two_blocks_in(m, name, None)
}

/// [`two_blocks`] in `comdat`.
fn two_blocks_in(m: &mut Module, name: &str, comdat: Option<ComdatId>) -> (SymId, Body) {
    let ty = m.types.function(VOID, &[], false).unwrap();
    let desc = FunctionDesc { has_body: true, comdat, ..FunctionDesc::new(ty) };
    let f = m.add_function(name.as_bytes(), &desc).unwrap();
    let mut x = Fx::new(m, f);
    let next = x.block();
    x.br(next);
    x.at(next);
    x.ret_void();
    (f, x.done())
}

/// `spec/ir.md` §9.6: what the module level cannot write is an error that
/// names the construct, never other text.
#[test]
fn module_errors_name_the_construct() {
    assert!(matches!(Dialect::new(18),
        Err(PrintError::Unsupported { ref construct, .. }) if construct == "dialect:llvm-18"));

    let mut m = small();
    m.set_module_flag(1, b"hod.other", FlagValue::Other).unwrap();
    assert_eq!(refusal(&m, 22, Part::Whole, &[]), "module-flag:hod.other");

    let mut m = small();
    m.props.frame_pointer = FramePointer::NonLeafNoReserve;
    for major in [19, 20, 21] {
        assert_eq!(refusal(&m, major, Part::Whole, &[]), "module-flag:frame-pointer=4");
    }
    assert!(prints(&m, 22, &[]));

    let mut m = small();
    let mut attrs = FnAttrs::new();
    let (_, token) =
        FnAttrs::FLAG_NAMES.into_iter().find(|(name, _)| *name == "sanitize_alloc_token").unwrap();
    attrs.set(token).unwrap();
    let attrs = m.intern_fn_attrs(&attrs).unwrap();
    let void_fn = m.types.function(VOID, &[], false).unwrap();
    m.add_function(b"f", &FunctionDesc { attrs, ..FunctionDesc::new(void_fn) }).unwrap();
    assert_eq!(refusal(&m, 19, Part::Whole, &[]), "attr:sanitize_alloc_token");
    assert!(prints(&m, 22, &[]));

    let mut m = small();
    m.record_unsupported("frontend:thing", None);
    assert_eq!(refusal(&m, 22, Part::Whole, &[]), "frontend:thing");

    let mut m = small();
    m.symbol_named(b"ghost").unwrap();
    assert_eq!(refusal(&m, 22, Part::Whole, &[]), "unresolved-symbol");

    let mut m = small();
    let far = m.types.ptr(270).unwrap();
    m.add_variable(b"far", &VariableDesc::new(far)).unwrap();
    assert_eq!(refusal(&m, 19, Part::Whole, &[]), "type:ptr addrspace(270)");
    assert!(prints(&m, 22, &[]));

    // A COMDAT with an external member on both sides of a split.
    let mut m = small();
    let comdat = m.comdat_named(b"c").unwrap();
    let (_, f_body) = two_blocks_in(&mut m, "f", Some(comdat));
    let (_, g_body) = two_blocks_in(&mut m, "g", Some(comdat));
    assert_eq!(refusal(&m, 22, Part::Main, &[&f_body]), "split:comdat");
    assert_eq!(refusal(&m, 22, Part::Rest, &[&g_body]), "split:comdat");
    assert!(prints(&m, 22, &[&f_body, &g_body]));

    // A `blockaddress` of a function the text only declares, and of an
    // entry block.
    let mut m = small();
    let (f, body) = two_blocks(&mut m, "f");
    let label = m.consts.block_addr(f, body.entry()).unwrap();
    m.add_variable(b"label", &VariableDesc { init: Some(label), ..VariableDesc::new(PTR) })
        .unwrap();
    assert_eq!(refusal(&m, 22, Part::Whole, &[&body]), "blockaddress-of-entry-block");
    let mut m = small();
    let (f, body) = two_blocks(&mut m, "f");
    let second = BlockId::new(1);
    let label = m.consts.block_addr(f, second).unwrap();
    m.add_variable(b"label", &VariableDesc { init: Some(label), ..VariableDesc::new(PTR) })
        .unwrap();
    assert_eq!(refusal(&m, 22, Part::Whole, &[]), "blockaddress-of-declaration");
    assert!(prints(&m, 22, &[&body]));
}

/// `spec/ir.md` §9.1: bodies that do not fit the module are `Malformed`.
#[test]
fn bodies_that_do_not_fit_are_malformed() {
    let mut m = small();
    let (_, body) = two_blocks(&mut m, "f");
    let malformed = |m: &Module, bodies: &[&Body]| {
        let result = m.print(&mut String::new(), Dialect::LLVM22, bodies.iter().copied());
        matches!(result, Err(PrintError::Malformed { .. }))
    };
    assert!(malformed(&m, &[&body, &body]), "a body twice");
    // The same body for a module whose symbol 0 is a variable.
    let mut other = small();
    other.add_variable(b"f", &VariableDesc::new(I32)).unwrap();
    assert!(malformed(&other, &[&body]), "a body for a variable");
    assert!(!malformed(&m, &[&body]));
}

/// A writer that refuses everything.
struct Full;

impl std::io::Write for Full {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("full"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// `spec/ir.md` §9.7: printing is deterministic, and the same through an
/// `io::Write` as into a `String`; a writer's error stops the printer.
#[test]
fn printing_is_deterministic() {
    for major in [22, 19] {
        let (m, bodies) = everything(major, false);
        let dialect = Dialect::new(major).unwrap();
        let first = printed(&m, dialect, &bodies);
        assert_eq!(first, printed(&m, dialect, &bodies), "LLVM {major}");
        let (again, again_bodies) = everything(major, false);
        assert_eq!(first, printed(&again, dialect, &again_bodies), "LLVM {major}: rebuilt");
        let mut sink = IoSink::new(Vec::new());
        m.print(&mut sink, dialect, &bodies).unwrap();
        assert_eq!(sink.finish().unwrap(), first.as_bytes(), "LLVM {major}: IoSink");
        let mut sink = IoSink::new(Full);
        assert_eq!(m.print(&mut sink, dialect, &bodies), Err(PrintError::Write));
        assert!(sink.finish().is_err());
    }
}
