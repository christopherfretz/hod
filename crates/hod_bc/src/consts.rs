//! Values and module-level constants (`spec/bc.md` §5.6).
//!
//! Value ids number the global values in record order, then the module
//! CONSTANTS block's constants. A constant is built when something at the
//! module level uses it — or when the function-block stage asks for it —
//! by a walk with its own stack over the constants it uses, each built
//! once; a constant that uses itself is refused.

use hod_ir::{BlockId, ConstId, ConstOp, Module, SymId, TypeId, TypeKind};

use crate::codes::cst;
use crate::error::{BcErrorKind, BcResult, fail, ir};
use crate::raw::Rec;
use crate::types::{NOTE_PTRAUTH, Types};

/// A global value: its symbol (none for the special arrays of
/// `spec/ll.md` §5.9), its address's type, and whether it is a function
/// with a body (for `blockaddress`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct GlobalValue {
    pub sym: Option<SymId>,
    pub ptr_ty: TypeId,
    pub defined_function: bool,
}

#[derive(Clone, Copy, Debug)]
enum Memo {
    New,
    Open,
    Done(ConstId, u8),
}

/// The value table of the module level.
#[derive(Debug, Default)]
pub(crate) struct Values {
    pub globals: Vec<GlobalValue>,
    /// Each module constant: its type, the notes its type makes, and its
    /// record (an index into `Raw::constants`).
    consts: Vec<(TypeId, u8, u32)>,
    memo: Vec<Memo>,
    ty_ids: Vec<TypeId>,
    ty_notes: Vec<u8>,
}

/// A walk's step: a constant, and the next of its operands to look at.
type Step = (usize, usize);

impl Values {
    /// The constants of `records` (SETTYPE included), after `globals`.
    pub fn new(
        globals: Vec<GlobalValue>,
        records: &[Rec],
        pool: &[u64],
        types: &Types,
    ) -> BcResult<Values> {
        let mut consts = Vec::new();
        let mut current = (TypeId::I32, 0u8);
        for (index, rec) in records.iter().enumerate() {
            if rec.code == cst::SETTYPE {
                let [id] = rec.ops(pool) else {
                    return fail(
                        BcErrorKind::Invalid,
                        rec.pos,
                        "a SETTYPE record without its type",
                    );
                };
                let ty = types.get(*id, rec.pos)?;
                if matches!(ty, TypeId::VOID | TypeId::LABEL | TypeId::METADATA) {
                    return fail(
                        BcErrorKind::Type,
                        rec.pos,
                        "a constant of a type no constant has",
                    );
                }
                current = (ty, types.notes_of(*id));
                continue;
            }
            consts.push((current.0, current.1, index as u32));
        }
        let memo = vec![Memo::New; consts.len()];
        Ok(Values {
            globals,
            consts,
            memo,
            ty_ids: types.ids.clone(),
            ty_notes: types.notes.clone(),
        })
    }

    /// The number of values.
    pub fn len(&self) -> usize {
        self.globals.len() + self.consts.len()
    }

    /// The type of value `id`.
    pub fn type_of(&self, id: u64) -> Option<TypeId> {
        let id = usize::try_from(id).ok()?;
        match self.globals.get(id) {
            Some(global) => Some(global.ptr_ty),
            None => self.consts.get(id - self.globals.len()).map(|c| c.0),
        }
    }

    /// The global value `id`, if it is one.
    pub fn global(&self, id: u64) -> Option<GlobalValue> {
        usize::try_from(id).ok().and_then(|id| self.globals.get(id)).copied()
    }

    /// Value `id` as a constant, built if it was not, with the notes it
    /// makes (`spec/bc.md` §5.6).
    pub fn constant(
        &mut self,
        module: &mut Module,
        records: &[Rec],
        pool: &[u64],
        id: u64,
        at: u64,
    ) -> BcResult<(ConstId, u8)> {
        let globals = self.globals.len();
        let Some(index) = usize::try_from(id).ok().filter(|&id| id < self.len()) else {
            return fail(
                BcErrorKind::Invalid,
                at,
                format!("value id {id} names no module-level value"),
            );
        };
        if index < globals {
            return Ok((self.address(module, index, at)?, 0));
        }
        let root = index - globals;
        if let Memo::Done(built, notes) = self.memo[root] {
            return Ok((built, notes));
        }
        let mut stack: Vec<Step> = vec![(root, 0)];
        self.memo[root] = Memo::Open;
        while let Some(&mut (c, ref mut next)) = stack.last_mut() {
            let rec = records[self.consts[c].2 as usize];
            let ops = rec.ops(pool);
            if let Some(dep) = operand(rec.code, ops, *next) {
                *next += 1;
                let Some(dep) = usize::try_from(dep).ok().filter(|&d| d < self.len()) else {
                    return fail(
                        BcErrorKind::Invalid,
                        rec.pos,
                        format!("value id {dep} names no module-level value"),
                    );
                };
                if dep < globals {
                    continue;
                }
                let dep = dep - globals;
                match self.memo[dep] {
                    Memo::Done(..) => {}
                    Memo::Open => {
                        return fail(BcErrorKind::Invalid, rec.pos, "a constant that uses itself");
                    }
                    Memo::New => {
                        self.memo[dep] = Memo::Open;
                        stack.push((dep, 0));
                    }
                }
                continue;
            }
            let built = self.build(module, rec, ops, c)?;
            self.memo[c] = Memo::Done(built.0, built.1);
            stack.pop();
        }
        match self.memo[root] {
            Memo::Done(built, notes) => Ok((built, notes)),
            _ => unreachable!("the walk built its root"),
        }
    }

    /// The address of global value `index`, as `hod_ll`'s `address_of`.
    fn address(&self, module: &mut Module, index: usize, at: u64) -> BcResult<ConstId> {
        let global = self.globals[index];
        let Some(sym) = global.sym else {
            return fail(BcErrorKind::Invalid, at, "a use of one of the special arrays");
        };
        let address = ir(at, module.consts.addr(sym, 0))?;
        if global.ptr_ty == TypeId::PTR {
            return Ok(address);
        }
        ir(
            at,
            module.consts.fold_cast(&module.types, ConstOp::AddrSpaceCast, address, global.ptr_ty),
        )
    }

    /// The built value `id`, whose constants are built.
    fn get(&self, module: &mut Module, id: u64, at: u64) -> BcResult<(ConstId, u8)> {
        let index = id as usize;
        if index < self.globals.len() {
            return Ok((self.address(module, index, at)?, 0));
        }
        match self.memo[index - self.globals.len()] {
            Memo::Done(built, notes) => Ok((built, notes)),
            _ => unreachable!("operands are built first"),
        }
    }

    fn sym_of(&self, id: u64, at: u64) -> BcResult<GlobalValue> {
        match self.global(id) {
            Some(global @ GlobalValue { sym: Some(_), .. }) => Ok(global),
            _ => fail(BcErrorKind::Type, at, "an operand that is not a global value"),
        }
    }

    /// Builds constant `c`, whose operands are built.
    #[allow(clippy::too_many_lines)] // One record table, read in one place.
    fn build(
        &self,
        module: &mut Module,
        rec: Rec,
        ops: &[u64],
        c: usize,
    ) -> BcResult<(ConstId, u8)> {
        let at = rec.pos;
        let (ty, mut notes, _) = self.consts[c];
        let short = || fail(BcErrorKind::Invalid, at, "a constant record cut short");
        let mut args: Vec<ConstId> = Vec::new();
        let mut take = |module: &mut Module, id: u64, args: &mut Vec<ConstId>| -> BcResult<()> {
            let (built, n) = self.get(module, id, at)?;
            notes |= n;
            args.push(built);
            Ok(())
        };
        let built: ConstId = match rec.code {
            cst::NULL => {
                let (consts, types) = (&mut module.consts, &module.types);
                ir(
                    at,
                    match types.kind(ty) {
                        TypeKind::Int(_) => consts.int(types, ty, 0),
                        TypeKind::Half
                        | TypeKind::BFloat
                        | TypeKind::Float
                        | TypeKind::Double
                        | TypeKind::Fp128
                        | TypeKind::X86Fp80 => consts.float(types, ty, 0),
                        TypeKind::Ptr { .. } => consts.null(types, ty),
                        TypeKind::Token => consts.none_token(),
                        _ => consts.zero(types, ty),
                    },
                )?
            }
            cst::UNDEF => ir(at, module.consts.undef(&module.types, ty))?,
            cst::POISON => ir(at, module.consts.poison(&module.types, ty))?,
            cst::INTEGER => {
                let [value] = ops else { return short() };
                let (consts, types) = (&mut module.consts, &module.types);
                let Some(bits) = types.int_bits(ty) else {
                    return fail(
                        BcErrorKind::Type,
                        at,
                        "an integer constant of a type not an integer",
                    );
                };
                let value = signed(*value);
                ir(
                    at,
                    if bits <= 64 {
                        let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
                        consts.int(types, ty, value as u64 & mask)
                    } else if value < 0 {
                        consts.int_signed(types, ty, value)
                    } else {
                        consts.int(types, ty, value as u64)
                    },
                )?
            }
            cst::WIDE_INTEGER => {
                let (consts, types) = (&mut module.consts, &module.types);
                let Some(bits) = types.int_bits(ty) else {
                    return fail(
                        BcErrorKind::Type,
                        at,
                        "an integer constant of a type not an integer",
                    );
                };
                let words = (bits as usize).div_ceil(64).max(1);
                if ops.len() > words {
                    return fail(
                        BcErrorKind::Invalid,
                        at,
                        "a wide integer with more words than its type",
                    );
                }
                let mut words_of: Vec<u64> = ops.iter().map(|&w| signed(w) as u64).collect();
                words_of.resize(words, 0);
                ir(at, consts.int_from_words(types, ty, &words_of))?
            }
            cst::FLOAT => {
                let (consts, types) = (&mut module.consts, &module.types);
                let bits = match (types.kind(ty), ops) {
                    (TypeKind::X86Fp80, [high, low]) => {
                        (u128::from(high >> 48) << 64) | u128::from((low & 0xffff) | (high << 16))
                    }
                    (TypeKind::Fp128, [low, high]) => (u128::from(*high) << 64) | u128::from(*low),
                    (_, [bits]) => u128::from(*bits),
                    _ => return short(),
                };
                ir(at, consts.float(types, ty, bits))?
            }
            cst::AGGREGATE => {
                for &id in ops {
                    take(module, id, &mut args)?;
                }
                if !matches!(
                    module.types.kind(ty),
                    TypeKind::Struct { .. } | TypeKind::Array { .. } | TypeKind::Vector { .. }
                ) {
                    return fail(
                        BcErrorKind::Type,
                        at,
                        "this aggregate does not have the type it is given",
                    );
                }
                ir(at, module.consts.aggregate(&module.types, ty, &args))?
            }
            cst::STRING | cst::CSTRING => {
                let mut bytes = crate::attrs::chars(ops, at)?;
                if rec.code == cst::CSTRING {
                    bytes.push(0);
                }
                let built = ir(at, module.consts.bytes(&mut module.types, &bytes))?;
                if module.consts.ty(built) != ty {
                    return fail(
                        BcErrorKind::Type,
                        at,
                        "a string constant whose type is not its length of i8",
                    );
                }
                built
            }
            cst::DATA => {
                let (consts, types) = (&mut module.consts, &module.types);
                let (elem, len) = match types.kind(ty) {
                    TypeKind::Array { elem, len } => (elem, len),
                    TypeKind::Vector { elem, len } => (elem, u64::from(len)),
                    _ => {
                        return fail(
                            BcErrorKind::Type,
                            at,
                            "a data constant of a type not an array or vector",
                        );
                    }
                };
                if len != ops.len() as u64 {
                    return fail(
                        BcErrorKind::Invalid,
                        at,
                        "a data constant with the wrong number of elements",
                    );
                }
                let float = !matches!(types.kind(elem), TypeKind::Int(_));
                for &value in ops {
                    args.push(if float {
                        ir(at, consts.float(types, elem, u128::from(value)))?
                    } else {
                        ir(at, consts.int(types, elem, value))?
                    });
                }
                ir(at, consts.aggregate(types, ty, &args))?
            }
            cst::CE_BINOP => {
                let [op, lhs, rhs, ..] = ops else { return short() };
                let op = match op {
                    0 => ConstOp::Add,
                    1 => ConstOp::Sub,
                    2 => ConstOp::Mul,
                    12 => ConstOp::Xor,
                    _ => {
                        return fail(
                            BcErrorKind::Unsupported("const-expr-binop"),
                            at,
                            format!("binary constant expression {op}"),
                        );
                    }
                };
                take(module, *lhs, &mut args)?;
                take(module, *rhs, &mut args)?;
                ir(at, module.consts.fold_binary(&module.types, op, args[0], args[1]))?
            }
            cst::CE_CAST => {
                let [op, _, value] = ops else { return short() };
                let op = match op {
                    0 => ConstOp::Trunc,
                    9 => ConstOp::PtrToInt,
                    10 => ConstOp::IntToPtr,
                    11 => ConstOp::BitCast,
                    12 => ConstOp::AddrSpaceCast,
                    13 => ConstOp::PtrToAddr,
                    _ => {
                        return fail(
                            BcErrorKind::Unsupported("const-expr-cast"),
                            at,
                            format!("cast constant expression {op}"),
                        );
                    }
                };
                take(module, *value, &mut args)?;
                ir(at, module.consts.fold_cast(&module.types, op, args[0], ty))?
            }
            cst::CE_GEP | cst::CE_GEP_WITH_INRANGE => {
                let (Some(first), Some(&source)) = (gep_operands(rec.code, ops), ops.first())
                else {
                    return short();
                };
                if first >= ops.len() || !(ops.len() - first).is_multiple_of(2) {
                    return short();
                }
                let source_ty = self.ty(source, at)?;
                let source_notes = self.ty_notes.get(source as usize).copied().unwrap_or(0);
                for pair in ops[first..].chunks(2) {
                    take(module, pair[1], &mut args)?;
                }
                notes |= source_notes;
                let m = &mut *module;
                ir(at, m.consts.fold_gep(&mut m.types, source_ty, args[0], &args[1..]))?
            }
            cst::CE_EXTRACTELT | cst::CE_INSERTELT | cst::CE_SHUFFLEVEC | cst::CE_SHUFVEC_EX => {
                let mut k = 0;
                while let Some(id) = operand(rec.code, ops, k) {
                    take(module, id, &mut args)?;
                    k += 1;
                }
                let op = match rec.code {
                    cst::CE_EXTRACTELT => ConstOp::ExtractElement,
                    cst::CE_INSERTELT => ConstOp::InsertElement,
                    _ => ConstOp::ShuffleVector,
                };
                let need = if rec.code == cst::CE_EXTRACTELT { 2 } else { 3 };
                if args.len() != need {
                    return short();
                }
                ir(at, module.consts.expr(op, ty, None, &args))?
            }
            cst::BLOCKADDRESS => {
                let [_, function, block] = ops else { return short() };
                let global = self.sym_of(*function, at)?;
                if !global.defined_function {
                    return fail(
                        BcErrorKind::Invalid,
                        at,
                        "blockaddress of a function this module does not define",
                    );
                }
                let Ok(block) = u32::try_from(*block) else { return short() };
                let sym = global.sym.expect("sym_of gives a symbol");
                ir(at, module.consts.block_addr(sym, BlockId::new(block)))?
            }
            cst::DSO_LOCAL_EQUIVALENT | cst::NO_CFI_VALUE => {
                let [_, value] = ops else { return short() };
                let sym = self.sym_of(*value, at)?.sym.expect("sym_of gives a symbol");
                ir(
                    at,
                    if rec.code == cst::NO_CFI_VALUE {
                        module.consts.no_cfi(sym)
                    } else {
                        module.consts.dso_local_equivalent(sym)
                    },
                )?
            }
            cst::PTRAUTH | cst::PTRAUTH2 => {
                let mut k = 0;
                while let Some(id) = operand(rec.code, ops, k) {
                    take(module, id, &mut args)?;
                    k += 1;
                }
                if args.len() < 4 {
                    return short();
                }
                notes |= NOTE_PTRAUTH;
                args[0]
            }
            cst::CE_SELECT => return unsupported(at, "const-expr-select"),
            cst::CE_CMP => return unsupported(at, "const-expr-cmp"),
            cst::CE_UNOP => return unsupported(at, "const-expr-unop"),
            cst::CE_GEP_OLD | cst::CE_INBOUNDS_GEP | cst::CE_GEP_WITH_INRANGE_INDEX_OLD => {
                return unsupported(at, "const-expr-gep-old");
            }
            cst::INLINEASM | cst::INLINEASM_OLD | cst::INLINEASM_OLD2 | cst::INLINEASM_OLD3 => {
                return unsupported(at, "module-inline-asm-value");
            }
            code => {
                return fail(
                    BcErrorKind::Unsupported("record"),
                    at,
                    format!("record code {code} in a CONSTANTS block"),
                );
            }
        };
        // As `hod_ll`'s `word_const`: an expression has the type it is
        // given.
        if module.consts.ty(built) != ty {
            return fail(BcErrorKind::Type, at, "this constant does not have the type it is given");
        }
        Ok((built, notes))
    }

    fn ty(&self, id: u64, at: u64) -> BcResult<TypeId> {
        match usize::try_from(id).ok().and_then(|i| self.ty_ids.get(i)) {
            Some(&ty) => Ok(ty),
            None => fail(BcErrorKind::Invalid, at, format!("type id {id} names no type")),
        }
    }
}

#[cold]
fn unsupported<T>(at: u64, name: &'static str) -> BcResult<T> {
    fail(BcErrorKind::Unsupported(name), at, format!("{name} at the module level"))
}

/// A sign-rotated VBR value (`decodeSignRotatedValue`).
pub(crate) fn signed(value: u64) -> i64 {
    if value & 1 == 0 {
        (value >> 1) as i64
    } else if value != 1 {
        -((value >> 1) as i64)
    } else {
        i64::MIN
    }
}

/// The operand position where a GEP's (type, value) pairs start.
fn gep_operands(code: u32, ops: &[u64]) -> Option<usize> {
    if code == cst::CE_GEP {
        return Some(2);
    }
    // [source, flags, width, lower, upper] or, wider than 64 bits,
    // [source, flags, width, words, lower…, upper…].
    let width = *ops.get(2)?;
    if width <= 64 {
        return Some(5);
    }
    let packed = *ops.get(3)?;
    Some(4 + (packed & 0xFFFF_FFFF) as usize + (packed >> 32) as usize)
}

/// The `k`th value id a constant record uses, in order.
fn operand(code: u32, ops: &[u64], k: usize) -> Option<u64> {
    let at = |positions: &[usize]| positions.get(k).and_then(|&p| ops.get(p)).copied();
    match code {
        cst::AGGREGATE => ops.get(k).copied(),
        cst::CE_BINOP => at(&[1, 2]),
        cst::CE_CAST => at(&[2]),
        cst::CE_GEP | cst::CE_GEP_WITH_INRANGE => {
            let first = gep_operands(code, ops)?;
            let p = first.checked_add(k.checked_mul(2)?)?.checked_add(1)?;
            ops.get(p).copied()
        }
        cst::CE_EXTRACTELT => at(&[1, 3]),
        cst::CE_INSERTELT => at(&[0, 1, 3]),
        cst::CE_SHUFFLEVEC => at(&[0, 1, 2]),
        cst::CE_SHUFVEC_EX => at(&[1, 2, 3]),
        cst::PTRAUTH => at(&[0, 1, 2, 3]),
        cst::PTRAUTH2 => at(&[0, 1, 2, 3, 4]),
        _ => None,
    }
}
