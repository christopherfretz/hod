//! What the `classify` tests share (`spec/ir.md` §8): small fixtures, and
//! [`program_for`], which builds for any row of the support matrix a
//! program that contains that construct and nothing else that could be
//! refused once everything is implemented.

#![allow(dead_code, clippy::many_single_char_names)]

use hod_ir::{
    AliasDesc, Arch, AtomicOrdering, BlockId, Body, BranchProtection, Bundle, CallConv, CallDesc,
    CallSite, Classifier, Clause, CodeModel, ComdatKind, ConstId, ConstOp, DiCompileUnit, DiFile,
    DiId, EmissionKind, FloatPredicate, FnAttrs, FnStr, FramePointer, FunctionBuilder,
    FunctionDesc, IFuncDesc, InlineAsm, IntPredicate, Intrinsic, Linkage, LocId, MemFlags, Module,
    Opcode, ParamAttrs, ParamMem, PicLevel, Rejection, Retained, RmwOp, StackArgs, Support, SymId,
    Symbol, SyncScope, TailKind, TargetSpec, TlsModel, TypeClass, TypeId, UnnamedAddr, UwTable,
    Value, VariableDesc, Visibility,
};

pub const I1: TypeId = TypeId::I1;
pub const I8: TypeId = TypeId::I8;
pub const I32: TypeId = TypeId::I32;
pub const I64: TypeId = TypeId::I64;
pub const FLOAT: TypeId = TypeId::FLOAT;
pub const DOUBLE: TypeId = TypeId::DOUBLE;
pub const PTR: TypeId = TypeId::PTR;
pub const VOID: TypeId = TypeId::VOID;

/// A module and the bodies of some of its functions.
#[derive(Debug)]
pub struct Program {
    pub m: Module,
    pub bodies: Vec<Body>,
}

/// A [`StackArgs`] with fixed answers.
#[derive(Debug, Clone, Copy)]
pub struct Stack {
    pub function: Option<u64>,
    pub call: Option<u64>,
}

impl StackArgs for Stack {
    fn of_function(&self, _: &Module, _: Symbol<'_>) -> Option<u64> {
        self.function
    }

    fn of_call(&self, _: &Module, _: &CallSite<'_>) -> Option<u64> {
        self.call
    }
}

/// Every signature needs no stack.
pub const NO_STACK: Stack = Stack { function: Some(0), call: Some(0) };

/// The first reason `program` is refused under `support`: the module
/// pass, then each body.
pub fn verdict(support: &Support, program: &Program) -> Result<(), Rejection> {
    let classifier = Classifier::new(support).with_stack_args(&NO_STACK);
    classifier.module(&program.m)?;
    for body in &program.bodies {
        classifier.body(&program.m, body)?;
    }
    Ok(())
}

pub fn target(arch: Arch) -> TargetSpec {
    match arch {
        Arch::Aarch64 => TargetSpec::aarch64_linux_gnu(),
        Arch::X86_64 => TargetSpec::x86_64_linux_gnu(),
    }
}

pub fn module(arch: Arch) -> Module {
    Module::new(target(arch))
}

pub fn everything(arch: Arch) -> Support {
    Support::everything_implemented(arch)
}

pub fn int(m: &mut Module, ty: TypeId, value: u64) -> Value {
    Value::constant(m.consts.int(&m.types, ty, value).unwrap()).unwrap()
}

pub fn constant(id: ConstId) -> Value {
    Value::constant(id).unwrap()
}

pub fn sym(id: SymId) -> Value {
    Value::sym(id).unwrap()
}

pub fn align(bytes: u64) -> MemFlags {
    MemFlags::aligned(bytes).unwrap()
}

pub fn atomic(bytes: u64) -> MemFlags {
    align(bytes).with_atomic(AtomicOrdering::SeqCst, SyncScope::System)
}

/// Declares the function `name` of type `ret (params)`.
pub fn declare(m: &mut Module, name: &str, ret: TypeId, params: &[TypeId]) -> (SymId, TypeId) {
    let ty = m.types.function(ret, params, false).unwrap();
    (m.add_function(name.as_bytes(), &FunctionDesc::new(ty)).unwrap(), ty)
}

/// Declares the function `name` of type `ret (params)` as one that gets
/// a body.
pub fn define(m: &mut Module, name: &str, ret: TypeId, params: &[TypeId]) -> (SymId, TypeId) {
    let ty = m.types.function(ret, params, false).unwrap();
    define_as(m, name, &FunctionDesc::new(ty))
}

pub fn define_as(m: &mut Module, name: &str, desc: &FunctionDesc<'_>) -> (SymId, TypeId) {
    let desc = FunctionDesc { has_body: true, ..*desc };
    (m.add_function(name.as_bytes(), &desc).unwrap(), desc.ty)
}

/// Declares the variable `name` of type `ty`.
pub fn variable(m: &mut Module, name: &str, ty: TypeId) -> SymId {
    m.add_variable(name.as_bytes(), &VariableDesc::new(ty)).unwrap()
}

/// Defines the variable `name` with the initializer `init`.
pub fn defined(m: &mut Module, name: &str, init: ConstId) -> SymId {
    let ty = m.consts.ty(init);
    m.add_variable(name.as_bytes(), &VariableDesc { init: Some(init), ..VariableDesc::new(ty) })
        .unwrap()
}

/// The body of `function`: an entry block that `build` fills. A block
/// `build` leaves open ends with `ret void`.
pub fn body(
    m: &Module,
    function: (SymId, TypeId),
    build: impl FnOnce(&mut FunctionBuilder, BlockId),
) -> Body {
    let mut b = FunctionBuilder::new(m, function.0, function.1).unwrap();
    let entry = b.create_block().unwrap();
    b.begin_block(entry).unwrap();
    build(&mut b, entry);
    if b.current_block().is_some() {
        b.ret(None).unwrap();
    }
    b.finish().unwrap()
}

/// A fresh block of `b`, begun, with the block before it branching there.
pub fn next_block(b: &mut FunctionBuilder) -> BlockId {
    let block = b.create_block().unwrap();
    b.br(block).unwrap();
    b.begin_block(block).unwrap();
    block
}

/// The module every body case starts from: `void f(i32, ptr, double)`
/// that gets a body, a declared `void callee()`, a declared variable `g`.
pub struct Fx {
    pub m: Module,
    pub f: (SymId, TypeId),
    pub callee: (SymId, TypeId),
    pub g: SymId,
}

pub fn fx(arch: Arch) -> Fx {
    let mut m = module(arch);
    let f = define(&mut m, "f", VOID, &[I32, PTR, DOUBLE]);
    let callee = declare(&mut m, "callee", VOID, &[]);
    let g = variable(&mut m, "g", I32);
    Fx { m, f, callee, g }
}

impl Fx {
    /// The program of `f` with the body `build` makes.
    pub fn with(self, build: impl FnOnce(&mut FunctionBuilder, BlockId)) -> Program {
        let body = body(&self.m, self.f, build);
        Program { m: self.m, bodies: vec![body] }
    }

    pub fn call(&self) -> CallDesc<'static> {
        CallDesc::new(sym(self.callee.0), self.callee.1, &[])
    }
}

/// A program of one function `void f(params)` described by `desc`, with
/// an empty body.
fn function_program<'a>(
    arch: Arch,
    params: &[TypeId],
    edit: impl FnOnce(&mut Module, &mut FunctionDesc<'a>),
) -> Program {
    let mut m = module(arch);
    let ty = m.types.function(VOID, params, false).unwrap();
    let mut desc = FunctionDesc::new(ty);
    edit(&mut m, &mut desc);
    let f = define_as(&mut m, "f", &desc);
    let body = body(&m, f, |_, _| {});
    Program { m, bodies: vec![body] }
}

fn attrs_program(arch: Arch, edit: impl FnOnce(&mut FnAttrs)) -> Program {
    function_program(arch, &[], |m, desc| {
        let mut attrs = FnAttrs::new();
        edit(&mut attrs);
        desc.attrs = m.intern_fn_attrs(&attrs).unwrap();
    })
}

fn param_program(arch: Arch, ty: TypeId, attrs: ParamAttrs) -> Program {
    let list = [attrs];
    function_program(arch, &[ty], |_, desc| desc.param_attrs = &list)
}

fn module_program(arch: Arch, edit: impl FnOnce(&mut Module)) -> Program {
    let mut m = module(arch);
    edit(&mut m);
    Program { m, bodies: Vec::new() }
}

fn variable_program(arch: Arch, edit: impl FnOnce(&mut Module, &mut VariableDesc<'_>)) -> Program {
    module_program(arch, |m| {
        let zero = m.consts.int(&m.types, I32, 0).unwrap();
        let mut desc = VariableDesc { init: Some(zero), ..VariableDesc::new(I32) };
        edit(m, &mut desc);
        m.add_variable(b"v", &desc).unwrap();
    })
}

/// A type of class `class`.
pub fn type_of_class(m: &mut Module, class: TypeClass) -> TypeId {
    match class {
        TypeClass::Int => I32,
        TypeClass::I128 => TypeId::I128,
        TypeClass::IntWide => m.types.int(256).unwrap(),
        TypeClass::Ptr => PTR,
        TypeClass::Half => TypeId::HALF,
        TypeClass::BFloat => TypeId::BFLOAT,
        TypeClass::Float => FLOAT,
        TypeClass::Double => DOUBLE,
        TypeClass::Fp128 => TypeId::FP128,
        TypeClass::X86Fp80 => TypeId::X86_FP80,
        TypeClass::Vector => m.types.vector(I32, 4).unwrap(),
        TypeClass::ScalableVector => m.types.scalable_vector(I32, 4).unwrap(),
        TypeClass::Aggregate => m.types.struct_type(&[I32, I64], false).unwrap(),
        TypeClass::Token => TypeId::TOKEN,
        TypeClass::TargetExt => m.types.target_ext("spirv.Image").unwrap(),
    }
}

/// The `ConstOp`s, in declaration order. `ConstOp` has no list of its
/// own; `tests/classify_matrix.rs` checks this one against the matrix.
pub const CONST_OPS: [ConstOp; 19] = [
    ConstOp::Trunc,
    ConstOp::ZExt,
    ConstOp::SExt,
    ConstOp::PtrToInt,
    ConstOp::PtrToAddr,
    ConstOp::IntToPtr,
    ConstOp::BitCast,
    ConstOp::AddrSpaceCast,
    ConstOp::GetElementPtr,
    ConstOp::Add,
    ConstOp::Sub,
    ConstOp::Mul,
    ConstOp::Xor,
    ConstOp::And,
    ConstOp::Or,
    ConstOp::Shl,
    ConstOp::ExtractElement,
    ConstOp::InsertElement,
    ConstOp::ShuffleVector,
];

/// LLVM's numbers of the calling conventions that have a keyword.
/// The conventions with a row, in the order of the rows.
pub const CC_NUMBERS: [u32; 26] = [
    0, 8, 9, 10, 13, 14, 15, 16, 17, 18, 19, 20, 21, 64, 65, 70, 77, 78, 79, 80, 81, 82, 83, 92,
    97, 98,
];

pub const PARAM_FLAGS: [(&str, u16); 7] = [
    ("zeroext", ParamAttrs::ZEROEXT),
    ("signext", ParamAttrs::SIGNEXT),
    ("inreg", ParamAttrs::INREG),
    ("nest", ParamAttrs::NEST),
    ("swiftself", ParamAttrs::SWIFT_SELF),
    ("swifterror", ParamAttrs::SWIFT_ERROR),
    ("swiftasync", ParamAttrs::SWIFT_ASYNC),
];

pub const PARAM_MEMS: [(&str, ParamMem); 6] = [
    ("sret", ParamMem::Sret),
    ("byval", ParamMem::ByVal),
    ("byref", ParamMem::ByRef),
    ("inalloca", ParamMem::InAlloca),
    ("preallocated", ParamMem::Preallocated),
    ("elementtype", ParamMem::ElementType),
];

const PROTECTION: [(&str, u8); 8] = [
    ("branch-target-enforcement", BranchProtection::BTI),
    ("guarded-control-stack", BranchProtection::GCS),
    ("sign-return-address", BranchProtection::PAC_RET),
    ("sign-return-address-all", BranchProtection::PAC_RET_ALL),
    ("sign-return-address-with-bkey", BranchProtection::PAC_B_KEY),
    ("branch-protection-pauth-lr", BranchProtection::PAC_PAUTH_LR),
    ("cf-protection-branch", BranchProtection::CET_IBT),
    ("cf-protection-return", BranchProtection::CET_SHSTK),
];

/// A program that contains the construct of matrix row `row` and, apart
/// from it, only constructs that are supported once everything is
/// implemented. Panics for a name that is not a row: the completeness
/// test relies on that.
#[allow(clippy::too_many_lines)]
pub fn program_for(row: &str, arch: Arch) -> Program {
    let (group, name) = row.split_once(':').expect("group:name");
    match group {
        "module" => module_row(name, arch),
        "structor" => module_program(arch, |m| {
            let (f, _) = declare(m, "ctor", VOID, &[]);
            match name {
                "priority" => m.add_ctor(101, f, None),
                "associated" => {
                    let g = variable(m, "g", I32);
                    m.add_dtor(65535, f, Some(g));
                }
                _ => panic!("no case for {row}"),
            }
        }),
        "debug" => match name {
            "module-flags" => module_program(arch, |m| m.props.dwarf_version = Some(5)),
            // A compile unit with `-gdwarf-3` (version 4 is written since
            // WP-114).
            "dwarf-version" => module_program(arch, |m| {
                m.props.dwarf_version = Some(3);
                let d = &mut m.debug;
                let file = DiFile { filename: b"a.c", directory: b"/", ..Default::default() };
                let file = d.add(&file.into()).unwrap();
                let unit = DiCompileUnit {
                    language: 0x1d,
                    file: Some(file),
                    emission_kind: EmissionKind::FullDebug,
                    ..Default::default()
                };
                let unit = d.add_distinct(&unit.into()).unwrap();
                d.add_compile_unit(unit).unwrap();
            }),
            "subprogram" => {
                function_program(arch, &[], |_, desc| desc.subprogram = Some(DiId::new(0)))
            }
            "slot-variables" => fx(arch).with(|b, _| {
                let slot = b.add_slot(4, 4).unwrap();
                b.set_slot_var(slot, DiId::new(0), LocId::new(0)).unwrap();
            }),
            "locations" => fx(arch).with(|b, _| b.set_loc(LocId::new(0))),
            _ => panic!("no case for {row}"),
        },
        "global" => global_row(name, arch),
        "linkage" => {
            let linkage =
                Linkage::ALL.into_iter().find(|linkage| linkage.keyword() == name).expect(row);
            module_program(arch, |m| {
                let ty = m.types.array(I32, 2).unwrap();
                let zero = m.consts.zero(&m.types, ty).unwrap();
                let init = (linkage != Linkage::ExternWeak).then_some(zero);
                let desc = VariableDesc { linkage, init, ..VariableDesc::new(ty) };
                m.add_variable(b"v", &desc).unwrap();
            })
        }
        "visibility" => variable_program(arch, |_, desc| {
            desc.visibility = match name {
                "hidden" => Visibility::Hidden,
                "protected" => Visibility::Protected,
                _ => panic!("no case for {row}"),
            };
        }),
        "tls" => variable_program(arch, |_, desc| {
            desc.tls = match name {
                "general-dynamic" => TlsModel::GeneralDynamic,
                "local-dynamic" => TlsModel::LocalDynamic,
                "initial-exec" => TlsModel::InitialExec,
                "local-exec" => TlsModel::LocalExec,
                _ => panic!("no case for {row}"),
            };
        }),
        "comdat" => {
            let kind = ComdatKind::ALL.into_iter().find(|kind| kind.keyword() == name).expect(row);
            variable_program(arch, |m, desc| {
                let comdat = m.comdat_named(b"v").unwrap();
                m.set_comdat_kind(comdat, kind);
                desc.comdat = Some(comdat);
            })
        }
        "init" => init_row(name, arch),
        "function" => match name {
            "parameters" => function_program(arch, &[I32], |_, _| {}),
            "variadic" => function_program(arch, &[], |m, desc| {
                desc.ty = m.types.function(VOID, &[], true).unwrap();
            }),
            "may-unwind" => function_program(arch, &[], |_, _| {}),
            "personality" => function_program(arch, &[], |m, desc| {
                desc.personality = Some(declare(m, "__gxx_personality_v0", I32, &[]).0);
            }),
            "slots" => fx(arch).with(|b, _| {
                b.add_slot(8, 8).unwrap();
            }),
            "over-aligned-slot" => fx(arch).with(|b, _| {
                b.add_slot(64, 64).unwrap();
            }),
            _ => panic!("no case for {row}"),
        },
        "cc" => {
            let cc = CC_NUMBERS
                .into_iter()
                .map(|number| CallConv::from_llvm(number).unwrap())
                .find(|cc| cc.keyword() == Some(name))
                .expect(row);
            function_program(arch, &[], |_, desc| desc.cc = cc)
        }
        "attr" => attr_row(name, arch),
        "type" => {
            let class = TypeClass::named(name).expect(row);
            let mut m = module(arch);
            let ty = type_of_class(&mut m, class);
            let f = define(&mut m, "f", VOID, &[ty]);
            let body = body(&m, f, |_, _| {});
            Program { m, bodies: vec![body] }
        }
        "inst" => {
            let op = Opcode::ALL.iter().copied().find(|op| op.name() == name).expect(row);
            inst_row(op, arch)
        }
        "mem" => {
            let fx = fx(arch);
            fx.with(|b, _| {
                let (a, p) = (b.arg(0).unwrap(), b.arg(1).unwrap());
                match name {
                    "volatile" => b.load(I32, p, align(4).with_volatile()),
                    "atomic-load" => b.load(I32, p, atomic(4)),
                    "atomic-store" => b.store(a, p, atomic(4)),
                    _ => panic!("no case for mem:{name}"),
                }
                .unwrap();
            })
        }
        "atomicrmw" => {
            let op = RmwOp::ALL.iter().copied().find(|op| op.name() == name).expect(row);
            fx(arch).with(|b, _| {
                let (a, p, d) = (b.arg(0).unwrap(), b.arg(1).unwrap(), b.arg(2).unwrap());
                if op.is_float() {
                    b.atomic_rmw(op, DOUBLE, p, d, atomic(8)).unwrap();
                } else {
                    b.atomic_rmw(op, I32, p, a, atomic(4)).unwrap();
                }
            })
        }
        "call" => call_row(name, arch),
        "tail" => {
            let fx = fx(arch);
            let tail = match name {
                "tail" => TailKind::Tail,
                "notail" => TailKind::NoTail,
                "musttail" => TailKind::MustTail,
                _ => panic!("no case for {row}"),
            };
            let desc = CallDesc { tail, ..fx.call() };
            fx.with(|b, _| {
                b.call(VOID, &desc).unwrap();
            })
        }
        "bundle" => {
            let fx = fx(arch);
            let bundles = [Bundle { tag: name, inputs: &[] }];
            let desc = CallDesc { bundles: &bundles, ..fx.call() };
            fx.with(|b, _| {
                b.call(VOID, &desc).unwrap();
            })
        }
        "landingpad" => {
            let mut fx = fx(arch);
            let null = m_null(&mut fx.m);
            let pad = fx.m.types.struct_type(&[PTR, I32], false).unwrap();
            fx.with(|b, _| {
                match name {
                    "cleanup" => b.landing_pad(pad, true, &[]),
                    "catch" => b.landing_pad(pad, false, &[Clause::Catch(null)]),
                    "filter" => b.landing_pad(pad, false, &[Clause::Filter(null)]),
                    _ => panic!("no case for landingpad:{name}"),
                }
                .unwrap();
            })
        }
        "operand" => operand_row(name, arch),
        "const-expr" => {
            let op = CONST_OPS.into_iter().find(|op| op.name() == name).expect(row);
            module_program(arch, |m| {
                let one = m.consts.int(&m.types, I32, 1).unwrap();
                let expr = m.consts.expr(op, I32, None, &[one]).unwrap();
                defined(m, "v", expr);
            })
        }
        "intrinsic" => {
            let intrinsic =
                Intrinsic::ALL.iter().copied().find(|intrinsic| intrinsic.name() == name);
            let intrinsic = intrinsic.expect(row);
            let fx = fx(arch);
            let overloads = [I32; 8];
            let overloads = &overloads[..intrinsic.overload_count()];
            let desc = CallDesc::intrinsic(intrinsic, overloads, fx.callee.1, &[]);
            fx.with(|b, _| {
                b.call(VOID, &desc).unwrap();
            })
        }
        _ => panic!("no case for {row}"),
    }
}

fn m_null(m: &mut Module) -> ConstId {
    m.consts.null(&m.types, PTR).unwrap()
}

fn module_row(name: &str, arch: Arch) -> Program {
    module_program(arch, |m| {
        if let Some(&(_, bit)) = PROTECTION.iter().find(|(key, _)| *key == name) {
            m.props.branch_protection.set(bit, true);
            return;
        }
        match name {
            "pic" => m.props.pic = PicLevel::Big,
            "pie" => m.props.pie = PicLevel::Small,
            "code-model-tiny" => m.props.code_model = Some(CodeModel::Tiny),
            "code-model-small" => m.props.code_model = Some(CodeModel::Small),
            "code-model-kernel" => m.props.code_model = Some(CodeModel::Kernel),
            "code-model-medium" => m.props.code_model = Some(CodeModel::Medium),
            "code-model-large" => m.props.code_model = Some(CodeModel::Large),
            "large-data-threshold" => m.props.large_data_threshold = Some(65536),
            "uwtable" => m.props.uwtable = UwTable::Async,
            "frame-pointer" => m.props.frame_pointer = FramePointer::All,
            "asm" => m.append_module_asm(b"  nop\n"),
            // WP-075: symbol directives only (`spec/ir.md` §2.3).
            "asm-symbols" => m.append_module_asm(b".globl x\n"),
            "linker-options" => m.add_linker_option(&[b"-lm"]).unwrap(),
            "dependent-libraries" => m.add_dependent_library(b"m").unwrap(),
            "global-ctors" => {
                let (f, _) = declare(m, "ctor", VOID, &[]);
                m.add_ctor(65535, f, None);
            }
            "global-dtors" => {
                let (f, _) = declare(m, "dtor", VOID, &[]);
                m.add_dtor(65535, f, None);
            }
            _ => panic!("no case for module:{name}"),
        }
    })
}

fn global_row(name: &str, arch: Arch) -> Program {
    match name {
        "function-declaration" => module_program(arch, |m| {
            declare(m, "f", VOID, &[]);
        }),
        "variable-declaration" => module_program(arch, |m| {
            variable(m, "g", I32);
        }),
        "function-definition" => function_program(arch, &[], |_, _| {}),
        "variable-definition" => variable_program(arch, |_, _| {}),
        "alias" | "alias-offset" | "alias-absolute" => module_program(arch, |m| {
            let zero = m.consts.int(&m.types, I64, 0).unwrap();
            let v = defined(m, "v", zero);
            let target = match name {
                "alias" => m.consts.addr(v, 0),
                "alias-offset" => m.consts.addr(v, 4),
                _ => m.consts.addr_of_int(4096),
            }
            .unwrap();
            m.add_alias(b"a", &AliasDesc::new(I32, target)).unwrap();
        }),
        "ifunc" => module_program(arch, |m| {
            let (resolver, ty) = declare(m, "resolver", PTR, &[]);
            m.add_ifunc(b"i", &IFuncDesc::new(ty, resolver)).unwrap();
        }),
        "section" => variable_program(arch, |_, desc| desc.section = Some(b".mydata")),
        "align" => variable_program(arch, |_, desc| desc.align = Some(64)),
        "unnamed-addr" => variable_program(arch, |_, desc| desc.unnamed_addr = UnnamedAddr::Global),
        "used" => module_program(arch, |m| {
            let zero = m.consts.int(&m.types, I32, 0).unwrap();
            let v = defined(m, "v", zero);
            m.retain(v, Retained::Used).unwrap();
        }),
        _ => panic!("no case for global:{name}"),
    }
}

/// A label difference (WP-095): block 0 of `f` less block 0 of `f`, as
/// a reader folds `sub (ptrtoint blockaddress, ptrtoint blockaddress)`.
fn block_diff(m: &mut Module, f: SymId) -> hod_ir::IrResult<ConstId> {
    let block = m.consts.block_addr(f, BlockId::new(0))?;
    let int = m.consts.int_of_addr(&m.types, I64, block)?;
    m.consts.fold_binary(&m.types, ConstOp::Sub, int, int)
}

/// `sub (i64 4, i64 ptrtoint @g)`: a number minus an address (WP-101).
fn neg_addr(m: &mut Module, g: SymId) -> hod_ir::IrResult<ConstId> {
    let addr = m.consts.addr(g, 0)?;
    let int = m.consts.int_of_addr(&m.types, I64, addr)?;
    let four = m.consts.int(&m.types, I64, 4)?;
    m.consts.fold_binary(&m.types, ConstOp::Sub, four, int)
}

fn init_row(name: &str, arch: Arch) -> Program {
    let mut m = module(arch);
    let g = variable(&mut m, "g", I32);
    let h = variable(&mut m, "h", I32);
    let mut bodies = Vec::new();
    let init = match name {
        "data" => m.consts.int(&m.types, I32, 7),
        "symbol" => m.consts.addr(g, 4),
        "blockaddress" => {
            let f = define(&mut m, "f", VOID, &[]);
            bodies.push(body(&m, f, |_, _| {}));
            m.consts.block_addr(f.0, BlockId::new(0))
        }
        "sym-diff" => m.consts.sym_diff(&m.types, I64, g, h, 0, false),
        "block-diff" => {
            let f = define(&mut m, "f", VOID, &[]);
            bodies.push(body(&m, f, |_, _| {}));
            block_diff(&mut m, f.0)
        }
        "sym-diff-plt" => m.consts.sym_diff(&m.types, I32, g, h, 0, true),
        "int-of-addr" => {
            let addr = m.consts.addr(g, 0).unwrap();
            m.consts.int_of_addr(&m.types, I64, addr)
        }
        "dso-local-equivalent" => m.consts.dso_local_equivalent(g),
        "no-cfi" => m.consts.no_cfi(g),
        "neg-addr" => neg_addr(&mut m, g),
        _ => panic!("no case for init:{name}"),
    }
    .unwrap();
    defined(&mut m, "v", init);
    Program { m, bodies }
}

fn attr_row(name: &str, arch: Arch) -> Program {
    if let Some(&(_, flag)) = PARAM_FLAGS.iter().find(|(key, _)| *key == name) {
        // `nest` and `swifterror` are on pointers, `zeroext` and `signext`
        // on integers (§5.8).
        let pointer = flag & (ParamAttrs::NEST | ParamAttrs::SWIFT_ERROR) != 0;
        return param_program(
            arch,
            if pointer { PTR } else { I32 },
            ParamAttrs::NONE.with(flag).unwrap(),
        );
    }
    if name == "elementtype" {
        // On an intrinsic's argument: a function's parameter may not
        // have it (§5.8).
        let mut fx = fx(arch);
        let fn_ty = fx.m.types.function(VOID, &[PTR], false).unwrap();
        let attrs = [ParamAttrs::NONE.with_mem(ParamMem::ElementType, I32).unwrap()];
        return fx.with(|b, _| {
            let p = b.arg(1).unwrap();
            let args = [p];
            let desc = CallDesc {
                arg_attrs: &attrs,
                ..CallDesc::intrinsic(Intrinsic::VaEnd, &[PTR], fn_ty, &args)
            };
            b.call(VOID, &desc).unwrap();
        });
    }
    if let Some(&(_, mem)) = PARAM_MEMS.iter().find(|(key, _)| *key == name) {
        return param_program(arch, PTR, ParamAttrs::NONE.with_mem(mem, I32).unwrap());
    }
    if let Some(&(_, flag)) = FnAttrs::FLAG_NAMES.iter().find(|(key, _)| *key == name) {
        return attrs_program(arch, |attrs| attrs.set(flag).unwrap());
    }
    match name {
        "align" => param_program(arch, PTR, ParamAttrs::NONE.with_align(8).unwrap()),
        "alignstack" => param_program(arch, PTR, ParamAttrs::NONE.with_stack_align(16).unwrap()),
        "uwtable" => attrs_program(arch, |attrs| attrs.set_uwtable(UwTable::Sync)),
        "no-builtin" => {
            attrs_program(arch, |attrs| attrs.set_string(FnStr::NoBuiltin, b"memcpy").unwrap())
        }
        _ => {
            let key = FnStr::ALL.into_iter().find(|key| key.name() == name);
            let key = key.unwrap_or_else(|| panic!("no case for attr:{name}"));
            attrs_program(arch, |attrs| attrs.set_string(key, b"1").unwrap())
        }
    }
}

fn call_row(name: &str, arch: Arch) -> Program {
    let mut fx = fx(arch);
    match name {
        "indirect" => {
            let ty = fx.callee.1;
            fx.with(|b, _| {
                let p = b.arg(1).unwrap();
                b.call(VOID, &CallDesc::new(p, ty, &[])).unwrap();
            })
        }
        "inline-asm" => {
            let ty = fx.callee.1;
            let asm = InlineAsm { template: b"nop", constraints: b"", flags: 0, ty };
            let asm = Value::asm(fx.m.inline_asm(&asm).unwrap()).unwrap();
            fx.with(|b, _| {
                b.call(VOID, &CallDesc::new(asm, ty, &[])).unwrap();
            })
        }
        // WP-075: a template that assembles to nothing.
        "inline-asm-empty" => {
            let ty = fx.callee.1;
            let asm = InlineAsm { template: b" ", constraints: b"~{memory}", flags: 1, ty };
            let asm = Value::asm(fx.m.inline_asm(&asm).unwrap()).unwrap();
            fx.with(|b, _| {
                b.call(VOID, &CallDesc::new(asm, ty, &[])).unwrap();
            })
        }
        "variadic" => {
            let ty = fx.m.types.function(I32, &[PTR], true).unwrap();
            let printf = fx.m.add_function(b"printf", &FunctionDesc::new(ty)).unwrap();
            fx.with(|b, _| {
                let (a, p) = (b.arg(0).unwrap(), b.arg(1).unwrap());
                b.call(I32, &CallDesc::new(sym(printf), ty, &[p, a])).unwrap();
            })
        }
        _ => panic!("no case for call:{name}"),
    }
}

fn operand_row(name: &str, arch: Arch) -> Program {
    let mut fx = fx(arch);
    let g = fx.g;
    let h = variable(&mut fx.m, "h", I32);
    let m = &mut fx.m;
    let pair = m.types.struct_type(&[PTR, I64], false).unwrap();
    let operand = match name {
        "symbol" => sym(g),
        "slot" => Value::NONE,
        "blockaddress" => constant(m.consts.block_addr(fx.f.0, BlockId::new(0)).unwrap()),
        "addr-of-int" => constant(m.consts.addr_of_int(4096).unwrap()),
        "int-of-addr" => {
            let addr = m.consts.addr(g, 0).unwrap();
            constant(m.consts.int_of_addr(&m.types, I64, addr).unwrap())
        }
        "sym-diff" => constant(m.consts.sym_diff(&m.types, I64, g, h, 0, false).unwrap()),
        "block-diff" => constant(block_diff(m, fx.f.0).unwrap()),
        "dso-local-equivalent" => constant(m.consts.dso_local_equivalent(fx.callee.0).unwrap()),
        "no-cfi" => constant(m.consts.no_cfi(fx.callee.0).unwrap()),
        "neg-addr" => constant(neg_addr(m, g).unwrap()),
        "address-in-aggregate" => {
            let addr = m.consts.addr(g, 0).unwrap();
            let zero = m.consts.int(&m.types, I64, 0).unwrap();
            constant(m.consts.aggregate(&m.types, pair, &[addr, zero]).unwrap())
        }
        _ => panic!("no case for operand:{name}"),
    };
    fx.with(|b, _| {
        let p = b.arg(1).unwrap();
        let operand = if operand.is_none() {
            let slot = b.add_slot(8, 8).unwrap();
            b.slot(slot).unwrap()
        } else {
            operand
        };
        // Stored, so that the instruction has no other rule about what
        // its operand is.
        b.store(operand, p, align(8)).unwrap();
    })
}

#[allow(clippy::too_many_lines)]
fn inst_row(op: Opcode, arch: Arch) -> Program {
    let mut fx = fx(arch);
    let m = &mut fx.m;
    let v4 = m.types.vector(I32, 4).unwrap();
    let pair = m.types.struct_type(&[I32, I1], false).unwrap();
    let vector = constant(m.consts.zero(&m.types, v4).unwrap());
    let agg = constant(m.consts.zero(&m.types, pair).unwrap());
    let one_id = m.consts.int(&m.types, I32, 1).unwrap();
    let truth = int(m, I1, 1);
    let call = fx.call();
    fx.with(|b, entry| {
        let (a, p, d) = (b.arg(0).unwrap(), b.arg(1).unwrap(), b.arg(2).unwrap());
        let result = match op {
            op if op.is_int_binary() => b.binary(op, I32, a, a),
            op if op.is_float_binary() => b.binary(op, DOUBLE, d, d),
            Opcode::FNeg => b.fneg(DOUBLE, d),
            Opcode::ICmp => b.icmp(IntPredicate::Eq, I1, a, a),
            Opcode::FCmp => b.fcmp(FloatPredicate::Oeq, I1, d, d),
            Opcode::Trunc => b.cast(op, I8, a),
            Opcode::ZExt | Opcode::SExt => b.cast(op, I64, a),
            Opcode::FPTrunc => b.cast(op, FLOAT, d),
            Opcode::FPExt => b.cast(op, TypeId::FP128, d),
            Opcode::FPToUI | Opcode::FPToSI => b.cast(op, I32, d),
            Opcode::UIToFP | Opcode::SIToFP => b.cast(op, DOUBLE, a),
            Opcode::PtrToInt | Opcode::PtrToAddr => b.cast(op, I64, p),
            Opcode::IntToPtr => b.cast(op, PTR, a),
            Opcode::BitCast => b.cast(op, I64, d),
            Opcode::AddrSpaceCast => b.cast(op, PTR, p),
            Opcode::Load => b.load(I32, p, align(4)),
            Opcode::Store => b.store(a, p, align(4)),
            Opcode::Alloca => b.alloca(PTR, a, 4, 4),
            Opcode::Fence => b.fence(AtomicOrdering::SeqCst, SyncScope::System),
            Opcode::CmpXchg => b.cmpxchg(pair, p, a, a, atomic(4), AtomicOrdering::SeqCst, false),
            Opcode::AtomicRmw => b.atomic_rmw(RmwOp::Add, I32, p, a, atomic(4)),
            Opcode::PtrAdd => b.ptr_add(PTR, p, 8, &[(a, 4)]),
            Opcode::ExtractValue => b.extract_value(I32, agg, 0),
            Opcode::InsertValue => b.insert_value(pair, agg, a, 0),
            Opcode::ExtractElement => b.extract_element(I32, vector, a),
            Opcode::InsertElement => b.insert_element(v4, vector, a, a),
            Opcode::ShuffleVector => b.shuffle_vector(v4, vector, vector, &[0, 1, 2, 3]),
            Opcode::Phi => {
                next_block(b);
                b.phi(I32, &[(a, entry)])
            }
            Opcode::Select => b.select(I32, truth, a, a),
            Opcode::Freeze => b.freeze(I32, a),
            Opcode::VaArg => b.va_arg(I32, p),
            Opcode::Call => b.call(VOID, &call),
            Opcode::LandingPad => b.landing_pad(pair, false, &[]),
            Opcode::CatchPad | Opcode::CleanupPad => b.funclet(op, TypeId::TOKEN),
            Opcode::Br => {
                next_block(b);
                b.ret(None)
            }
            Opcode::Ret => b.ret(None),
            Opcode::Unreachable => b.unreachable(),
            Opcode::Resume => b.resume(agg),
            Opcode::CatchSwitch | Opcode::CatchRet | Opcode::CleanupRet => b.funclet(op, VOID),
            Opcode::CondBr
            | Opcode::Switch
            | Opcode::IndirectBr
            | Opcode::CallBr
            | Opcode::Invoke => {
                let (then, other) = (b.create_block().unwrap(), b.create_block().unwrap());
                match op {
                    Opcode::CondBr => b.cond_br(truth, then, other),
                    Opcode::Switch => b.switch(a, other, &[(one_id, then)]),
                    Opcode::IndirectBr => b.indirect_br(p, &[then, other]),
                    Opcode::CallBr => b.callbr(VOID, &call, then, &[other]),
                    _ => b.invoke(VOID, &call, then, other),
                }
                .unwrap();
                b.begin_block(then).unwrap();
                b.ret(None).unwrap();
                b.begin_block(other).unwrap();
                b.unreachable()
            }
            other => panic!("no case for inst:{}", other.name()),
        };
        result.unwrap();
    })
}
