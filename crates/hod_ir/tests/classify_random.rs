//! `classify` on random input (`spec/ir.md` §8.1, §8.7 to §8.9): a
//! deterministic generator builds modules and bodies through the builder
//! API, over every opcode, and `classify` must decide every one of them
//! without panicking.
//!
//! - "tame" programs name only what exists. With everything implemented
//!   they are never malformed, and what is refused is refused by design;
//!   under the shipped matrix, the census is what repeated
//!   classification finds, in the same order.
//! - "wild" programs also use ids that name nothing (symbols, constants,
//!   types, inline assembly, attribute sets, blocks) and bodies given to
//!   a module they were not built for. Those are the malformed-input
//!   outcomes of §8.7, and never a panic.

// Random numbers are cut to size all over this file.
#![allow(clippy::cast_possible_truncation)]

mod classify_support;

use std::collections::BTreeSet;

use classify_support::{
    CC_NUMBERS, DOUBLE, FLOAT, I1, I8, I32, I64, NO_STACK, PARAM_FLAGS, PTR, Program, VOID,
    everything, module, verdict,
};
use hod_ir::{
    AliasDesc, Arch, AsmId, AtomicOrdering, BlockId, Body, Bundle, CallConv, CallDesc, Census,
    Classifier, Clause, CodeModel, ConstId, ConstOp, DiId, FlagValue, FloatPredicate, FnAttrs,
    FnAttrsId, FnStr, FunctionBuilder, FunctionDesc, IFuncDesc, INVALID_RULES, InlineAsm,
    IntPredicate, Intrinsic, IrResult, Linkage, LocId, MemFlags, Module, Opcode, ParamAttrs,
    PicLevel, Rejection, RejectionKind, RmwOp, STRUCTURAL_RULES, SUPPORT_ROWS, Support,
    SupportStatus, SymId, SyncScope, TailKind, TlsModel, TypeId, TypeKind, UwTable, Value,
    VariableDesc, Visibility,
};

/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    /// True once in `n` times.
    fn chance(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

/// A module and what the bodies built for it may name.
struct World {
    m: Module,
    value_types: Vec<TypeId>,
    fn_types: Vec<TypeId>,
    functions: Vec<(SymId, TypeId)>,
    variables: Vec<SymId>,
    pair: TypeId,
    pad: TypeId,
    vec4: TypeId,
    asm: Value,
    wild: bool,
}

fn signature(m: &Module, fn_ty: TypeId) -> (TypeId, Vec<TypeId>, bool) {
    if fn_ty.index() < m.types.len()
        && let TypeKind::Function { ret, params, variadic } = m.types.kind(fn_ty)
    {
        return (ret, params.to_vec(), variadic);
    }
    (VOID, Vec::new(), false)
}

fn random_cc(rng: &mut Rng, wild: bool) -> CallConv {
    let number = match rng.below(12) {
        0..=7 => 0,
        8 if wild => rng.below(1024) as u32,
        _ => rng.pick(&CC_NUMBERS),
    };
    CallConv::from_llvm(number).unwrap()
}

fn random_param_attrs(rng: &mut Rng, ty: TypeId) -> ParamAttrs {
    let mut attrs = ParamAttrs::NONE;
    if rng.chance(2) {
        attrs = attrs.with(rng.pick(&PARAM_FLAGS).1).unwrap_or(attrs);
    }
    if ty == PTR && rng.chance(3) {
        let mem = rng.pick(&classify_support::PARAM_MEMS).1;
        attrs = attrs.with_mem(mem, I64).unwrap_or(attrs);
    }
    if ty == PTR && rng.chance(4) {
        attrs = attrs.with_align(16).unwrap_or(attrs);
    }
    attrs
}

#[allow(clippy::too_many_lines)]
fn world(rng: &mut Rng, arch: Arch, wild: bool) -> World {
    let mut m = module(arch);
    let vec4 = m.types.vector(I32, 4).unwrap();
    let pair = m.types.struct_type(&[I32, I64], false).unwrap();
    let pad = m.types.struct_type(&[PTR, I32], false).unwrap();
    let array = m.types.array(I8, 16).unwrap();
    let rare = [
        TypeId::HALF,
        TypeId::BFLOAT,
        TypeId::FP128,
        TypeId::X86_FP80,
        TypeId::TOKEN,
        m.types.int(256).unwrap(),
        m.types.scalable_vector(I32, 4).unwrap(),
        m.types.ptr(1).unwrap(),
        m.types.target_ext("spirv.Image").unwrap(),
    ];
    let mut value_types =
        vec![I1, I8, I32, I32, I64, I64, PTR, PTR, FLOAT, DOUBLE, DOUBLE, TypeId::I128, vec4, pair];
    value_types.push(array);
    if rng.chance(3) {
        value_types.push(rng.pick(&rare));
    }

    let plain = m.types.function(VOID, &[], false).unwrap();
    let mut fn_types = vec![plain];
    for _ in 0..6 {
        let ret = if rng.chance(2) { VOID } else { rng.pick(&value_types) };
        let params: Vec<TypeId> = (0..rng.below(5)).map(|_| rng.pick(&value_types)).collect();
        fn_types.push(m.types.function(ret, &params, rng.chance(8)).unwrap_or(plain));
    }

    let props = &mut m.props;
    if rng.chance(4) {
        props.pic = PicLevel::Big;
    }
    if rng.chance(6) {
        props.uwtable = UwTable::Async;
    }
    if rng.chance(8) {
        props.code_model = Some(rng.pick(&[CodeModel::Small, CodeModel::Large, CodeModel::Tiny]));
    }
    if rng.chance(8) {
        props.branch_protection.set(1 << rng.below(8), true);
    }
    if rng.chance(8) {
        props.dwarf_version = Some(5);
    }
    if rng.chance(10) {
        m.set_module_flag(1, b"mystery", FlagValue::Int(1)).unwrap();
    }
    if rng.chance(10) {
        m.set_module_flag(1, b"wchar_size", FlagValue::Int(4)).unwrap();
    }
    if rng.chance(10) {
        m.append_module_asm(b".globl x\n");
    }
    if rng.chance(10) {
        m.add_linker_option(&[b"-lm"]).unwrap();
    }

    let personality = m.add_function(b"__gxx_personality_v0", &FunctionDesc::new(plain)).unwrap();
    let mut functions = Vec::new();
    for (index, &ty) in fn_types.iter().enumerate() {
        let sym = m.symbol_named(format!("f{index}").as_bytes()).unwrap();
        let has_body = index < 3;
        let (_, params, _) = signature(&m, ty);
        let mut attrs = FnAttrs::new();
        for (_, flag) in FnAttrs::FLAG_NAMES {
            // `naked` rarely: it is refused by design and would hide the rest.
            if rng.chance(if flag == FnAttrs::NAKED { 200 } else { 12 }) {
                let _ = attrs.set(flag);
            }
        }
        if rng.chance(3) {
            let _ = attrs.set(FnAttrs::NOUNWIND);
        }
        if rng.chance(8) {
            attrs.set_uwtable(UwTable::Sync);
        }
        for key in FnStr::ALL {
            if rng.chance(25) {
                let _ = attrs.set_string(key, b"x");
            }
        }
        let param_attrs: Vec<ParamAttrs> = if rng.chance(3) {
            params.iter().map(|&param| random_param_attrs(rng, param)).collect()
        } else {
            Vec::new()
        };
        let linkage = if has_body {
            rng.pick(&[Linkage::External, Linkage::External, Linkage::Internal, Linkage::WeakAny])
        } else {
            rng.pick(&[Linkage::External, Linkage::External, Linkage::ExternWeak])
        };
        let hidden = linkage == Linkage::External && rng.chance(5);
        let attrs = if wild && rng.chance(10) {
            FnAttrsId::new(rng.below(500) as u32)
        } else {
            m.intern_fn_attrs(&attrs).unwrap()
        };
        let desc = FunctionDesc {
            linkage,
            visibility: if hidden { Visibility::Hidden } else { Visibility::Default },
            section: (has_body && rng.chance(10)).then_some(b".text.mine"),
            cc: random_cc(rng, wild),
            attrs,
            param_attrs: &param_attrs,
            personality: rng.chance(6).then_some(personality),
            subprogram: rng.chance(12).then_some(DiId::new(0)),
            has_body,
            ..FunctionDesc::new(ty)
        };
        if m.set_function(sym, &desc).is_err() {
            m.set_function(sym, &FunctionDesc { has_body, ..FunctionDesc::new(ty) }).unwrap();
        }
        functions.push((sym, ty));
    }

    let mut variables: Vec<SymId> = Vec::new();
    for index in 0..6 {
        let sym = m.symbol_named(format!("g{index}").as_bytes()).unwrap();
        let ty = rng.pick(&[I32, I64, PTR, PTR, pair, array, DOUBLE]);
        let init = match rng.below(6) {
            0 | 1 => None,
            2 if ty == PTR && !variables.is_empty() => {
                Some(m.consts.addr(rng.pick(&variables), 8).unwrap())
            }
            3 if ty == PTR => Some(m.consts.addr(rng.pick(&functions).0, 0).unwrap()),
            4 if ty == PTR => {
                let function = rng.pick(&functions[..3]).0;
                Some(m.consts.block_addr(function, BlockId::new(0)).unwrap())
            }
            5 if ty == PTR && wild => {
                let nothing = SymId::new(1000 + rng.below(1000) as u32);
                Some(if rng.chance(2) {
                    m.consts.addr(nothing, 0).unwrap()
                } else {
                    m.consts.block_addr(rng.pick(&[nothing, personality]), BlockId::new(3)).unwrap()
                })
            }
            5 if ty == I64 && variables.len() > 1 => Some(
                m.consts.sym_diff(&m.types, I64, variables[0], variables[1], 4, false).unwrap(),
            ),
            _ => Some(m.consts.zero(&m.types, ty).unwrap()),
        };
        let tls = if rng.chance(5) { TlsModel::InitialExec } else { TlsModel::None };
        let desc = VariableDesc {
            linkage: rng.pick(&[Linkage::External, Linkage::External, Linkage::Internal]),
            section: (init.is_some() && rng.chance(8)).then_some(b".mydata"),
            align: rng.chance(8).then_some(64),
            tls,
            init,
            ..VariableDesc::new(ty)
        };
        if m.set_variable(sym, &desc).is_err() {
            m.set_variable(sym, &VariableDesc::new(ty)).unwrap();
        }
        variables.push(sym);
    }

    if rng.chance(4) {
        let sym = m.symbol_named(b"alias").unwrap();
        let target = m.consts.addr(rng.pick(&functions[..3]).0, 0).unwrap();
        if m.set_alias(sym, &AliasDesc::new(plain, target)).is_err() {
            m.set_variable(sym, &VariableDesc::new(I32)).unwrap();
        }
    }
    if rng.chance(6) {
        let sym = m.symbol_named(b"ifunc").unwrap();
        if m.set_ifunc(sym, &IFuncDesc::new(plain, functions[0].0)).is_err() {
            m.set_variable(sym, &VariableDesc::new(I32)).unwrap();
        }
    }
    if rng.chance(5) {
        m.add_ctor(rng.pick(&[65535, 101]), functions[0].0, None);
    }
    if rng.chance(20) {
        m.record_unsupported("attr:jumptable", Some(functions[0].0));
    }
    if wild && rng.chance(6) {
        m.symbol_named(b"ghost").unwrap();
    }

    let asm = InlineAsm { template: b"nop", constraints: b"", flags: 0, ty: plain };
    let asm = Value::asm(m.inline_asm(&asm).unwrap()).unwrap();
    World { m, value_types, fn_types, functions, variables, pair, pad, vec4, asm, wild }
}

/// How a call ends.
#[derive(Clone, Copy)]
enum How {
    Call,
    Invoke(BlockId, BlockId),
    CallBr(BlockId, BlockId),
}

struct Gen<'w> {
    rng: &'w mut Rng,
    w: &'w mut World,
    b: FunctionBuilder,
    function: SymId,
    values: Vec<(Value, TypeId)>,
    blocks: Vec<BlockId>,
}

const INT_OPS: [Opcode; 13] = [
    Opcode::Add,
    Opcode::Sub,
    Opcode::Mul,
    Opcode::UDiv,
    Opcode::SDiv,
    Opcode::URem,
    Opcode::SRem,
    Opcode::Shl,
    Opcode::LShr,
    Opcode::AShr,
    Opcode::And,
    Opcode::Or,
    Opcode::Xor,
];

const FLOAT_OPS: [Opcode; 5] =
    [Opcode::FAdd, Opcode::FSub, Opcode::FMul, Opcode::FDiv, Opcode::FRem];

const CASTS: [(Opcode, TypeId, TypeId); 14] = [
    (Opcode::Trunc, I64, I32),
    (Opcode::ZExt, I32, I64),
    (Opcode::SExt, I8, I32),
    (Opcode::FPTrunc, DOUBLE, FLOAT),
    (Opcode::FPExt, FLOAT, DOUBLE),
    (Opcode::FPToUI, DOUBLE, I32),
    (Opcode::FPToSI, FLOAT, I64),
    (Opcode::UIToFP, I32, DOUBLE),
    (Opcode::SIToFP, I64, FLOAT),
    (Opcode::PtrToInt, PTR, I64),
    (Opcode::PtrToAddr, PTR, I64),
    (Opcode::IntToPtr, I64, PTR),
    (Opcode::BitCast, DOUBLE, I64),
    (Opcode::AddrSpaceCast, PTR, PTR),
];

const TAGS: [&str; 5] = ["deopt", "funclet", "kcfi", "ptrauth", "mystery"];

impl Gen<'_> {
    fn wild(&mut self, n: usize) -> bool {
        self.w.wild && self.rng.chance(n)
    }

    /// A type for a result.
    fn ty(&mut self) -> TypeId {
        if self.wild(12) {
            return TypeId::new(self.rng.below(3000) as u32);
        }
        self.rng.pick(&self.w.value_types)
    }

    fn garbage(&mut self) -> Value {
        match self.rng.below(3) {
            0 => Value::sym(SymId::new(self.rng.below(3000) as u32)).unwrap(),
            1 => Value::constant(ConstId::new(self.rng.below(100_000) as u32)).unwrap(),
            _ => Value::sym(SymId::new(1 << 20)).unwrap(),
        }
    }

    fn constant(id: IrResult<ConstId>) -> Value {
        Value::constant(id.unwrap()).unwrap()
    }

    /// Some address.
    fn address(&mut self) -> Value {
        let m = &mut self.w.m;
        let variable = self.rng.pick(&self.w.variables);
        let function = self.rng.pick(&self.w.functions).0;
        match self.rng.below(12) {
            2 => Value::sym(function).unwrap(),
            3 => {
                let align = self.rng.pick(&[8, 16, 64]);
                match self.b.add_slot(16, align) {
                    Ok(slot) => {
                        if self.rng.chance(6) {
                            let _ = self.b.set_slot_var(slot, DiId::new(0), LocId::new(0));
                        }
                        self.b.slot(slot).unwrap()
                    }
                    Err(_) => Value::sym(variable).unwrap(),
                }
            }
            4 => {
                let id = m.consts.addr(variable, 8);
                Self::constant(id)
            }
            5 => {
                let id = m.consts.addr_of_int(4096);
                Self::constant(id)
            }
            6 => {
                let block = self.rng.below(self.blocks.len() + usize::from(self.w.wild));
                let other = self.w.functions[0].0;
                let function = if self.rng.chance(6) { other } else { self.function };
                let id = m.consts.block_addr(function, BlockId::new(block as u32));
                Self::constant(id)
            }
            7 => {
                let id = m.consts.null(&m.types, PTR);
                Self::constant(id)
            }
            8 => {
                let id = m.consts.dso_local_equivalent(function);
                Self::constant(id)
            }
            9 => {
                let id = m.consts.no_cfi(function);
                Self::constant(id)
            }
            10 => {
                let base = m.consts.addr(variable, 0).unwrap();
                let id = m.consts.expr(ConstOp::GetElementPtr, PTR, Some(I8), &[base]);
                match id {
                    Ok(id) => Value::constant(id).unwrap(),
                    Err(_) => Value::sym(variable).unwrap(),
                }
            }
            _ => Value::sym(variable).unwrap(),
        }
    }

    /// A value of type `ty`, or in a wild program maybe of nothing.
    fn value(&mut self, ty: TypeId) -> Value {
        if self.wild(8) {
            return self.garbage();
        }
        let have: Vec<Value> =
            self.values.iter().filter(|(_, have)| *have == ty).map(|(value, _)| *value).collect();
        if !have.is_empty() && !self.rng.chance(4) {
            return self.rng.pick(&have);
        }
        if ty == PTR {
            return self.address();
        }
        let m = &mut self.w.m;
        if ty.index() >= m.types.len() {
            let id = m.consts.int(&m.types, I32, 0);
            return Self::constant(id);
        }
        if ty == I64 && self.rng.chance(4) {
            let (a, b) = (self.rng.pick(&self.w.variables), self.rng.pick(&self.w.variables));
            let id = if self.rng.chance(2) {
                let addr = m.consts.addr(a, 0).unwrap();
                m.consts.int_of_addr(&m.types, I64, addr)
            } else {
                m.consts.sym_diff(&m.types, I64, a, b, 0, false)
            };
            if let Ok(id) = id {
                return Value::constant(id).unwrap();
            }
        }
        let id = match self.rng.below(3) {
            0 => m.consts.undef(&m.types, ty),
            1 => m.consts.poison(&m.types, ty),
            _ => m.consts.zero(&m.types, ty),
        };
        if let Ok(id) = id {
            Value::constant(id).unwrap()
        } else {
            Self::constant(m.consts.int(&m.types, I32, 0))
        }
    }

    fn mem(&mut self, atomic: bool) -> MemFlags {
        let mut mem = MemFlags::aligned(self.rng.pick(&[1, 4, 8])).unwrap();
        if self.rng.chance(6) {
            mem = mem.with_volatile();
        }
        if atomic || self.rng.chance(6) {
            let scope = self.rng.pick(&[SyncScope::System, SyncScope::SingleThread]);
            mem = mem.with_atomic(self.rng.pick(AtomicOrdering::ALL), scope);
        }
        mem
    }

    fn call(&mut self, how: How) -> (IrResult<Value>, TypeId) {
        let known = self.rng.pick(Intrinsic::ALL);
        let intrinsic = match self.rng.below(8) {
            0 | 1 => Some(known),
            2 => Some(Intrinsic::Other("llvm.mystery.call")),
            3 => Some(Intrinsic::ThreadLocalAddress),
            _ => None,
        };
        let (direct, mut fn_ty) = self.rng.pick(&self.w.functions);
        let mut callee = Value::sym(direct).unwrap();
        match self.rng.below(8) {
            0 => {
                callee = self.value(PTR);
                fn_ty = self.rng.pick(&self.w.fn_types);
            }
            1 => {
                (callee, fn_ty) = (self.w.asm, self.w.fn_types[0]);
                if self.wild(3) {
                    callee = Value::asm(AsmId::new(self.rng.below(50) as u32)).unwrap();
                }
            }
            2 => callee = Value::sym(self.rng.pick(&self.w.variables)).unwrap(),
            _ => {}
        }
        if intrinsic == Some(Intrinsic::ThreadLocalAddress) {
            fn_ty = self.w.m.types.function(PTR, &[PTR], false).unwrap();
        }
        if self.wild(12) {
            fn_ty = TypeId::new(self.rng.below(3000) as u32);
        }
        let (ret, params, variadic) = signature(&self.w.m, fn_ty);
        let mut args: Vec<Value> = params.iter().map(|&param| self.value(param)).collect();
        if variadic {
            args.push(self.value(I32));
        }
        let arg_attrs: Vec<ParamAttrs> = if self.rng.chance(8) {
            (0..args.len()).map(|_| random_param_attrs(self.rng, PTR)).collect()
        } else {
            Vec::new()
        };
        let inputs = [self.value(I32)];
        let bundles = [Bundle { tag: self.rng.pick(&TAGS), inputs: &inputs }];
        let overloads = [I32; 8];
        let mut desc = match intrinsic {
            Some(intrinsic) => {
                let overloads = &overloads[..intrinsic.overload_count()];
                CallDesc::intrinsic(intrinsic, overloads, fn_ty, &args)
            }
            None => CallDesc::new(callee, fn_ty, &args),
        };
        desc.cc = if self.rng.chance(4) { random_cc(self.rng, self.w.wild) } else { desc.cc };
        desc.tail = self.rng.pick(&[
            TailKind::None,
            TailKind::None,
            TailKind::Tail,
            TailKind::NoTail,
            TailKind::MustTail,
        ]);
        desc.returns_twice = self.rng.chance(20);
        desc.nounwind = self.rng.chance(2);
        desc.arg_attrs = &arg_attrs;
        if self.rng.chance(10) {
            desc.bundles = &bundles;
        }
        let result = match how {
            How::Call => self.b.call(ret, &desc),
            How::Invoke(normal, unwind) => self.b.invoke(ret, &desc, normal, unwind),
            How::CallBr(fallthrough, indirect) => {
                self.b.callbr(ret, &desc, fallthrough, &[indirect])
            }
        };
        (result, ret)
    }

    #[allow(clippy::too_many_lines)]
    fn inst(&mut self) {
        let (result, ty) = match self.rng.below(26) {
            0..=2 => {
                let ty = self.rng.pick(&[I8, I32, I64, TypeId::I128, self.w.vec4]);
                let (lhs, rhs) = (self.value(ty), self.value(ty));
                (self.b.binary(self.rng.pick(&INT_OPS), ty, lhs, rhs), ty)
            }
            3 => {
                let ty = self.rng.pick(&[FLOAT, DOUBLE]);
                let (lhs, rhs) = (self.value(ty), self.value(ty));
                (self.b.binary(self.rng.pick(&FLOAT_OPS), ty, lhs, rhs), ty)
            }
            4 => {
                let arg = self.value(DOUBLE);
                (self.b.fneg(DOUBLE, arg), DOUBLE)
            }
            5 => {
                let ty = self.rng.pick(&[I32, I64, PTR]);
                let (lhs, rhs) = (self.value(ty), self.value(ty));
                (self.b.icmp(self.rng.pick(IntPredicate::ALL), I1, lhs, rhs), I1)
            }
            6 => {
                let (lhs, rhs) = (self.value(DOUBLE), self.value(DOUBLE));
                (self.b.fcmp(self.rng.pick(FloatPredicate::ALL), I1, lhs, rhs), I1)
            }
            7 | 8 => {
                let (op, from, to) = self.rng.pick(&CASTS);
                let arg = self.value(from);
                (self.b.cast(op, to, arg), to)
            }
            9 | 10 => {
                let (ty, ptr, mem) = (self.ty(), self.value(PTR), self.mem(false));
                (self.b.load(ty, ptr, mem), ty)
            }
            11 | 12 => {
                let ty = self.rng.pick(&self.w.value_types);
                let (value, ptr, mem) = (self.value(ty), self.value(PTR), self.mem(false));
                (self.b.store(value, ptr, mem), VOID)
            }
            13 => {
                let count = self.value(I64);
                (self.b.alloca(PTR, count, 8, 16), PTR)
            }
            14 => {
                let ordering = self.rng.pick(AtomicOrdering::ALL);
                (self.b.fence(ordering, SyncScope::System), VOID)
            }
            15 => {
                let ty = self.rng.pick(&[I32, I64, PTR]);
                let pair = self.w.m.types.struct_type(&[ty, I1], false).unwrap();
                let (ptr, old, new) = (self.value(PTR), self.value(ty), self.value(ty));
                let (mem, failure) = (self.mem(true), self.rng.pick(AtomicOrdering::ALL));
                (self.b.cmpxchg(pair, ptr, old, new, mem, failure, self.rng.chance(2)), pair)
            }
            16 => {
                let op = self.rng.pick(RmwOp::ALL);
                let ty = if op.is_float() { DOUBLE } else { self.rng.pick(&[I32, I64]) };
                let (ptr, value, mem) = (self.value(PTR), self.value(ty), self.mem(true));
                (self.b.atomic_rmw(op, ty, ptr, value, mem), ty)
            }
            17 => {
                let (base, index) = (self.value(PTR), self.value(I64));
                (self.b.ptr_add(PTR, base, 8, &[(index, 4)]), PTR)
            }
            18 => {
                let pair = self.w.pair;
                let (agg, field) = (self.value(pair), self.value(I32));
                if self.rng.chance(2) {
                    (self.b.extract_value(I32, agg, 0), I32)
                } else {
                    (self.b.insert_value(pair, agg, field, 0), pair)
                }
            }
            19 => {
                let vec4 = self.w.vec4;
                let (vector, element, index) = (self.value(vec4), self.value(I32), self.value(I32));
                match self.rng.below(3) {
                    0 => (self.b.extract_element(I32, vector, index), I32),
                    1 => (self.b.insert_element(vec4, vector, element, index), vec4),
                    _ => (self.b.shuffle_vector(vec4, vector, vector, &[3, 2, 1, 0]), vec4),
                }
            }
            20 => {
                let ty = self.ty();
                let (cond, lhs, rhs) = (self.value(I1), self.value(ty), self.value(ty));
                (self.b.select(ty, cond, lhs, rhs), ty)
            }
            21 => {
                let ty = self.ty();
                let arg = self.value(ty);
                (self.b.freeze(ty, arg), ty)
            }
            22 if self.rng.chance(4) => {
                let list = self.value(PTR);
                (self.b.va_arg(I32, list), I32)
            }
            23 if self.rng.chance(3) => {
                let pad = self.w.pad;
                let m = &mut self.w.m;
                let mut clause = m.consts.addr(self.rng.pick(&self.w.variables), 0).unwrap();
                if self.wild(4) {
                    clause = ConstId::new(self.rng.below(100_000) as u32);
                }
                let clauses = [Clause::Catch(clause), Clause::Filter(clause)];
                let clauses = &clauses[..self.rng.below(3)];
                (self.b.landing_pad(pad, self.rng.chance(2), clauses), pad)
            }
            24 if self.rng.chance(8) => {
                let op = self.rng.pick(&[Opcode::CatchPad, Opcode::CleanupPad]);
                (self.b.funclet(op, TypeId::TOKEN), TypeId::TOKEN)
            }
            _ => self.call(How::Call),
        };
        if let Ok(value) = result
            && ty != VOID
            && self.b.current_block().is_some()
        {
            self.values.push((value, ty));
        }
    }

    fn terminate(&mut self, index: usize) {
        let next = self.blocks.get(index + 1).copied();
        let any = self.rng.pick(&self.blocks);
        let choice = self.rng.below(10);
        let _ = match (next, choice) {
            (None, 0) => self.b.unreachable(),
            (None, 1) => {
                let value = self.value(self.w.pad);
                self.b.resume(value)
            }
            (None, 2) => {
                let op =
                    self.rng.pick(&[Opcode::CatchRet, Opcode::CleanupRet, Opcode::CatchSwitch]);
                self.b.funclet(op, VOID)
            }
            (None, _) => {
                let (ret, ..) = signature(&self.w.m, self.b_type());
                let value = (ret != VOID).then(|| self.value(ret));
                self.b.ret(value)
            }
            (Some(next), 0) => {
                let cond = self.value(I1);
                self.b.cond_br(cond, next, any)
            }
            (Some(next), 1) => {
                let m = &mut self.w.m;
                let mut case = m.consts.int(&m.types, I32, 7).unwrap();
                if self.wild(3) {
                    case = ConstId::new(self.rng.below(100_000) as u32);
                }
                let on = self.value(I32);
                self.b.switch(on, next, &[(case, any)])
            }
            (Some(next), 2) => {
                let address = self.value(PTR);
                self.b.indirect_br(address, &[next, any])
            }
            (Some(next), 3) => self.call(How::Invoke(next, any)).0,
            (Some(next), 4) if self.rng.chance(5) => self.call(How::CallBr(next, any)).0,
            (Some(next), _) => self.b.br(next),
        };
        if self.b.current_block().is_some() {
            match next {
                Some(next) => self.b.br(next).unwrap(),
                None => self.b.unreachable().unwrap(),
            };
        }
    }

    fn b_type(&self) -> TypeId {
        self.w.functions.iter().find(|(sym, _)| *sym == self.function).unwrap().1
    }
}

/// The body of function `index` of `w`.
fn random_body(rng: &mut Rng, w: &mut World, index: usize) -> Option<Body> {
    let (function, fn_ty) = w.functions[index];
    let mut b = FunctionBuilder::new(&w.m, function, fn_ty).unwrap();
    let blocks: Vec<BlockId> = (0..=rng.below(5)).map(|_| b.create_block().unwrap()).collect();
    let (_, params, _) = signature(&w.m, fn_ty);
    let values: Vec<(Value, TypeId)> =
        params.iter().enumerate().map(|(at, &ty)| (b.arg(at as u32).unwrap(), ty)).collect();
    let mut generator = Gen { rng, w, b, function, values, blocks };
    if generator.rng.chance(6) {
        generator.b.set_loc(LocId::new(1));
    }
    for index in 0..generator.blocks.len() {
        generator.b.begin_block(generator.blocks[index]).unwrap();
        if index > 0 && generator.rng.chance(3) {
            let ty = generator.rng.pick(&[I32, PTR, DOUBLE]);
            let value = generator.value(ty);
            if let Ok(phi) = generator.b.phi(ty, &[(value, generator.blocks[0])]) {
                generator.values.push((phi, ty));
            }
        }
        for _ in 0..generator.rng.below(9) {
            if generator.b.current_block().is_none() {
                break;
            }
            generator.inst();
        }
        if generator.b.current_block().is_some() {
            generator.terminate(index);
        }
    }
    generator.b.finish().ok()
}

fn program(seed: u64, arch: Arch, wild: bool) -> Program {
    let mut rng = Rng(seed);
    let mut w = world(&mut rng, arch, wild);
    let bodies: Vec<Body> =
        (0..3).filter_map(|index| random_body(&mut rng, &mut w, index)).collect();
    Program { m: w.m, bodies }
}

fn census_of(support: &Support, program: &Program) -> Census {
    let classifier = Classifier::new(support).with_stack_args(&NO_STACK);
    let mut census = Census::new();
    classifier.census_module(&program.m, &mut census);
    for body in &program.bodies {
        classifier.census_body(&program.m, body, &mut census);
    }
    census
}

fn first(census: &Census) -> Option<(String, RejectionKind)> {
    census.entries().first().map(|entry| (entry.construct.clone(), entry.kind))
}

fn reason(result: Result<(), Rejection>) -> Option<(String, RejectionKind)> {
    result.err().map(|rejection| (rejection.construct.into_owned(), rejection.kind))
}

const SEEDS: u64 = 400;
const ARCHES: [Arch; 2] = [Arch::Aarch64, Arch::X86_64];

/// What repeated classification found (`found`) less the pairs the
/// census (`names`) cannot have: a pair `row/class` whose class's `type:`
/// row is not supported is hidden behind that row (§8.3, §8.9). The
/// iteration meets it once it has switched the type row on, so each pair
/// left out must come after its `type:` row, which the census has.
fn behind_type_rows<'f>(seed: u64, found: &'f [String], names: &[&str]) -> Vec<&'f str> {
    let mut kept = Vec::new();
    for (index, name) in found.iter().enumerate() {
        match name.rsplit_once('/') {
            Some((_, class)) if !names.contains(&name.as_str()) => {
                let type_row = format!("type:{class}");
                assert!(
                    found[..index].contains(&type_row) && names.contains(&type_row.as_str()),
                    "seed {seed}: {name} is in neither the census nor behind {type_row}"
                );
            }
            _ => kept.push(name.as_str()),
        }
    }
    kept
}

/// §8.1, §8.9: tame programs are never malformed. With everything
/// implemented, whatever is refused cannot be switched on; under the
/// shipped matrix the census lists what repeated classification finds,
/// in the same order, up to the first reason that cannot be switched on.
#[test]
fn tame_programs_are_decided_consistently() {
    let mut opcodes = BTreeSet::new();
    let (mut bodies, mut accepted, mut insts) = (0, 0, 0);
    for seed in 0..SEEDS {
        let arch = ARCHES[(seed % 2) as usize];
        let program = program(seed, arch, false);
        bodies += program.bodies.len();
        for body in &program.bodies {
            insts += body.inst_count();
            opcodes.extend(body.insts().map(|inst| inst.opcode().name()));
        }

        let all = everything(arch);
        let census = census_of(&all, &program);
        assert_eq!(first(&census), reason(verdict(&all, &program)), "seed {seed}");
        accepted += usize::from(census.is_empty());
        for entry in census.entries() {
            let name = entry.construct.as_str();
            assert_eq!(entry.kind, RejectionKind::Unsupported, "seed {seed}: {name}");
            assert_ne!(all.supports(name), Some(true), "seed {seed}: {name}");
            if let Some(row) = SUPPORT_ROWS.iter().find(|row| row.name == name) {
                let status = row.cell(arch).status;
                assert!(matches!(status, SupportStatus::Rejected(_)), "seed {seed}: {name}");
            }
        }

        let shipped = Support::shipped(arch);
        let census = census_of(shipped, &program);
        let names: Vec<&str> = census.entries().iter().map(|e| e.construct.as_str()).collect();
        let mut support = shipped.clone();
        let mut found: Vec<String> = Vec::new();
        while let Err(rejection) = verdict(&support, &program) {
            assert_eq!(rejection.kind, RejectionKind::Unsupported, "seed {seed}: {rejection}");
            let name = rejection.construct.into_owned();
            found.push(name.clone());
            match support.clone().with(&name) {
                Some(wider) => support = wider,
                None => break,
            }
            assert!(found.len() < 1000, "seed {seed}: no progress");
        }
        let found = behind_type_rows(seed, &found, &names);
        assert_eq!(found, names[..found.len()], "seed {seed}");
        if verdict(&support, &program).is_ok() {
            assert_eq!(found.len(), names.len(), "seed {seed}");
        }
    }
    // The generator is worth its name: bodies finish, some programs are
    // accepted whole, and every opcode is met.
    assert!(bodies as u64 > SEEDS * 2, "{bodies} bodies");
    assert!(accepted > 0 && insts > 5000, "{accepted} accepted, {insts} instructions");
    let missing: Vec<&str> =
        Opcode::ALL.iter().map(|op| op.name()).filter(|name| !opcodes.contains(name)).collect();
    assert!(missing.is_empty(), "never generated: {missing:?}");
}

/// §8.7: wild programs name things that do not exist. `classify` and
/// the census decide them without panicking, the two agree on the first
/// reason of each pass, and a malformed-input outcome is one of the
/// published ones.
#[test]
fn wild_programs_never_panic() {
    let mut invalid = BTreeSet::new();
    let mut structural = BTreeSet::new();
    let mut previous: Option<Program> = None;
    for seed in 0..SEEDS {
        let arch = ARCHES[(seed % 2) as usize];
        let program = program(0x5eed_0000 + seed, arch, true);
        for support in [Support::shipped(arch).clone(), everything(arch)] {
            // With and without the hook, each pass alone.
            for classifier in
                [Classifier::new(&support), Classifier::new(&support).with_stack_args(&NO_STACK)]
            {
                let mut census = Census::new();
                classifier.census_module(&program.m, &mut census);
                assert_eq!(first(&census), reason(classifier.module(&program.m)), "seed {seed}");
                for body in &program.bodies {
                    let mut census = Census::new();
                    classifier.census_body(&program.m, body, &mut census);
                    let result = classifier.body(&program.m, body);
                    assert_eq!(first(&census), reason(result), "seed {seed}");
                }
            }
            for entry in census_of(&support, &program).entries() {
                let name = entry.construct.as_str();
                let listed = INVALID_RULES.iter().any(|rule| rule.name == name);
                assert_eq!(entry.kind == RejectionKind::Invalid, listed, "seed {seed}: {name}");
                if listed {
                    invalid.insert(name.to_owned());
                }
                if STRUCTURAL_RULES.iter().any(|rule| rule.name == name) {
                    structural.insert(name.to_owned());
                }
            }
        }
        // A body classified against a module it was not built for.
        if let Some(previous) = &previous {
            let support = everything(arch);
            let classifier = Classifier::new(&support);
            for body in &previous.bodies {
                let mut census = Census::new();
                classifier.census_body(&program.m, body, &mut census);
                assert_eq!(first(&census), reason(classifier.body(&program.m, body)));
                let _ = hod_ir::classify_body(&program.m, body);
            }
            let _ = hod_ir::census(&program.m, &previous.bodies);
        }
        previous = Some(program);
    }
    for name in [
        "invalid:symbol",
        "invalid:constant",
        "invalid:type",
        "invalid:inline-asm",
        "invalid:blockaddress",
        "invalid:unresolved-symbol",
        "invalid:function-attributes",
    ] {
        assert!(invalid.contains(name), "{name} was never met: {invalid:?}");
    }
    for name in ["tls:direct-operand", "blockaddress:other-function", "addrspace:pointer"] {
        assert!(structural.contains(name), "{name} was never met: {structural:?}");
    }
}

/// WP-027, the finding of WP-033: with `function:parameters` supported
/// for some classes but not `token`, and `type:token` not supported (the
/// matrix of that day), seed 360 had a body with a `token` parameter (since
/// WP-179 added a function flag and a string key, which move the
/// generator's draws, seed 1289 is the one that has it). Its
/// finding is `type:token` (§8.3); the pair `function:parameters/token`
/// is a finding only once `type:token` is supported. The census and
/// repeated classification follow that one rule.
#[test]
fn a_pair_is_found_only_behind_its_type_row() {
    const PAIR: &str = "function:parameters/token";
    let seed = 1289;
    let arch = ARCHES[(seed % 2) as usize];
    let program = program(seed, arch, false);
    let then = Support::shipped(arch).clone().without("type:token").unwrap();
    let then = then.without("function:parameters/token").unwrap();
    assert_eq!(then.supports("function:parameters"), Some(true));

    let census = census_of(&then, &program);
    let names: Vec<&str> = census.entries().iter().map(|e| e.construct.as_str()).collect();
    assert!(names.contains(&"type:token") && !names.contains(&PAIR), "{names:?}");

    let token_on = then.clone().with("type:token").unwrap();
    let census = census_of(&token_on, &program);
    let names: Vec<&str> = census.entries().iter().map(|e| e.construct.as_str()).collect();
    assert!(names.contains(&PAIR) && !names.contains(&"type:token"), "{names:?}");

    let mut support = then;
    let mut found = Vec::new();
    while let Err(rejection) = verdict(&support, &program) {
        let name = rejection.construct.into_owned();
        match support.clone().with(&name) {
            Some(wider) => support = wider,
            None => break,
        }
        found.push(name);
    }
    let at = |name: &str| found.iter().position(|found| found == name);
    assert!(at("type:token").unwrap() < at(PAIR).unwrap(), "{found:?}");
}
