//! A generator of executable random programs (`spec/interp.md` §9).
//!
//! [`generate`] maps a seed and [`Options`] to a module, the bodies of its
//! functions, an entry function and arguments for it. The same inputs give
//! the same program. Every program is built through `hod_ir`'s public
//! builder, passes `Body::verify`, and runs to a result without a trap in
//! at most [`Generated::fuel_bound`] steps: each operation that could trap
//! is given operands that cannot make it (§9.2).

use std::collections::BTreeMap;

use hod_ir::{
    BlockId, Body, CallDesc, Clause, FloatPredicate, FunctionBuilder, FunctionDesc, IntPredicate,
    IrResult, MemFlags, Module, Opcode, SymId, TargetSpec, TypeId, Value, VariableDesc,
    resolve_intrinsic,
};

use crate::eh::{self, Eh};
use crate::host::{self, Standard};
use crate::interp::{Config, Interp, Outcome, Program};
use crate::value::{Val, mask};

/// What a generated program may contain (§9.3). Every switch that is off
/// removes its constructs entirely.
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct Options {
    /// The target of the module.
    pub target: TargetSpec,
    /// Functions in the program, the entry function included. At least 1.
    pub functions: u32,
    /// Roughly how many instructions a function has.
    pub insts: u32,
    /// The most steps a program may need: loops and calls are left out
    /// where they would take a run past it.
    pub max_fuel: u64,
    /// `i1`, `i8`, `i16` and `i32` besides `i64`. Needs `casts`.
    pub narrow_ints: bool,
    /// `i7` and `i33`. Needs `casts`.
    pub odd_ints: bool,
    /// `i128`. Needs `casts`.
    pub i128: bool,
    /// `float` and `double`.
    pub floats: bool,
    /// `icmp`, as a value and feeding branches. Control flow needs it.
    pub compares: bool,
    /// `select`. Needs `compares`.
    pub selects: bool,
    /// `trunc`, `zext` and `sext`.
    pub casts: bool,
    /// Stack slots, `load`, `store` and `ptradd`.
    pub memory: bool,
    /// Module variables with initializers. Needs `memory`.
    pub globals: bool,
    /// `ptrtoint` and `inttoptr`. Needs `memory`.
    pub ptr_int: bool,
    /// Integer and memory intrinsics.
    pub intrinsics: bool,
    /// Diamonds, triangles with a critical edge, a `condbr` with both
    /// edges to one block, blocks that end in a never-executed
    /// `unreachable`. Needs `compares`.
    pub branches: bool,
    /// Counted loops with `phi`s, a swap cycle among them, and blocks
    /// that are their own successors. Needs `compares`.
    pub loops: bool,
    /// Dense and sparse `switch`es, with cases that share a destination.
    /// Needs `compares`.
    pub switches: bool,
    /// Blocks without predecessors.
    pub unreachable_blocks: bool,
    /// Direct calls between the generated functions.
    pub calls: bool,
    /// Calls through function pointers. Needs `calls`, and `selects` or
    /// `globals` for where the pointer comes from.
    pub indirect_calls: bool,
    /// Functions with nine to twelve parameters. Needs `calls`.
    pub many_args: bool,
    /// Calls of the standard host functions `sink` and `source`.
    pub hosts: bool,
    /// C++ exceptions (§5.9): throws of `hod_throw`, `invoke`s of it and
    /// of the generated functions, landing pads with catch, catch-all,
    /// filter and cleanup clauses, `llvm.eh.typeid.for`, `resume`, and
    /// `nounwind` calls. Needs `compares`. Off by default, so that the
    /// programs of the seeds from before it stay the same.
    pub exceptions: bool,
    /// Computed gotos (§5.5): a constant table of `blockaddress`es, an
    /// `indirectbr` through it, and a loaded address compared with
    /// `null` (the one comparison LLVM defines). Needs `globals` and `compares`. Off by
    /// default, as `exceptions`.
    pub block_addresses: bool,
    /// Vectors (§5.10) of the integer widths in use, of 1 to 16 lanes:
    /// built from scalars by `insertelement` and `shufflevector`,
    /// element-wise arithmetic, compares and `select`s (with `compares`
    /// and `selects`), casts (with `casts`), shuffles and the vector
    /// intrinsics, and back to a scalar by `extractelement`, a reduction
    /// or a `bitcast`. Off by default, as `exceptions`, and not in
    /// [`Options::everything`].
    pub vectors: bool,
}

impl Default for Options {
    /// Everything on, four functions of about sixty instructions.
    fn default() -> Options {
        Options {
            target: TargetSpec::aarch64_linux_gnu(),
            functions: 4,
            insts: 60,
            max_fuel: 200_000,
            narrow_ints: true,
            odd_ints: true,
            i128: true,
            floats: true,
            compares: true,
            selects: true,
            casts: true,
            memory: true,
            globals: true,
            ptr_int: true,
            intrinsics: true,
            branches: true,
            loops: true,
            switches: true,
            unreachable_blocks: true,
            calls: true,
            indirect_calls: true,
            many_args: true,
            hosts: true,
            exceptions: false,
            block_addresses: false,
            vectors: false,
        }
    }
}

impl Options {
    /// Everything off: one function of straight-line `i64` arithmetic.
    pub fn minimal() -> Options {
        Options {
            functions: 1,
            narrow_ints: false,
            odd_ints: false,
            i128: false,
            floats: false,
            compares: false,
            selects: false,
            casts: false,
            memory: false,
            globals: false,
            ptr_int: false,
            intrinsics: false,
            branches: false,
            loops: false,
            switches: false,
            unreachable_blocks: false,
            calls: false,
            indirect_calls: false,
            many_args: false,
            hosts: false,
            ..Options::default()
        }
    }

    /// The default options with `exceptions` and `block_addresses` on:
    /// everything the generator makes but `vectors`.
    pub fn everything() -> Options {
        Options { exceptions: true, block_addresses: true, ..Options::default() }
    }

    /// These options with the size knobs set.
    #[must_use]
    pub fn sized(mut self, functions: u32, insts: u32) -> Options {
        self.functions = functions;
        self.insts = insts;
        self
    }

    /// The options with every switch off whose needs are not met (§9.3).
    #[must_use]
    pub fn effective(&self) -> Options {
        let mut o = self.clone();
        o.functions = o.functions.max(1);
        o.narrow_ints &= o.casts;
        o.odd_ints &= o.casts;
        o.i128 &= o.casts;
        o.selects &= o.compares;
        o.branches &= o.compares;
        o.loops &= o.compares;
        o.switches &= o.compares;
        o.globals &= o.memory;
        o.ptr_int &= o.memory;
        o.indirect_calls &= o.calls && (o.selects || o.globals);
        o.many_args &= o.calls;
        o.exceptions &= o.compares;
        o.block_addresses &= o.globals && o.compares;
        if !o.calls {
            o.functions = 1;
        }
        o
    }
}

/// How often each construct occurs in a program (§9.4): a map from the
/// names in [`Coverage::NAMES`] to counts.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Coverage(BTreeMap<&'static str, u64>);

impl Coverage {
    /// Every counter, in the order of §9.4.
    pub const NAMES: &'static [&'static str] = &[
        "int:i1",
        "int:i8",
        "int:i16",
        "int:i32",
        "int:i64",
        "int:i7",
        "int:i33",
        "int:i128",
        "float:op",
        "float:fcmp",
        "float:convert",
        "icmp:value",
        "icmp:branch",
        "select",
        "cast:trunc",
        "cast:zext",
        "cast:sext",
        "cast:ptrtoint",
        "cast:inttoptr",
        "freeze",
        "intrinsic:int",
        "intrinsic:checked",
        "intrinsic:mem",
        "mem:slot",
        "mem:load",
        "mem:store",
        "mem:odd-width",
        "ptradd:const",
        "ptradd:scaled",
        "global:load",
        "global:store",
        "cf:diamond",
        "cf:critical-edge",
        "cf:condbr-twice",
        "cf:dead-trap",
        "cf:loop",
        "cf:self-loop",
        "cf:swap",
        "cf:switch-dense",
        "cf:switch-sparse",
        "cf:switch-shared",
        "cf:unreachable-block",
        "phi",
        "call:direct",
        "call:indirect",
        "call:many-args",
        "call:unused-result",
        "call:sink",
        "call:source",
        "use:dead",
        "use:next",
        "use:once-later",
        "use:many",
        "use:other-block",
        "use:across-call",
    ];

    /// The counters of the constructs of `exceptions` and
    /// `block_addresses` (§9.4), which the default options leave out.
    pub const UNWIND_NAMES: &'static [&'static str] = &[
        "eh:throw",
        "eh:invoke-throw",
        "eh:invoke-call",
        "eh:shared-pad",
        "eh:pad-phi",
        "eh:catch",
        "eh:catch-all",
        "eh:filter",
        "eh:filter-empty",
        "eh:cleanup",
        "eh:typeid-for",
        "eh:nounwind-call",
        "eh:normal-phi",
        "cf:indirectbr",
        "blockaddress:table",
        "blockaddress:compare",
    ];

    /// The counters of the constructs of `vectors` (§9.4), which the
    /// default options leave out.
    pub const VECTOR_NAMES: &'static [&'static str] = &[
        "vector:build",
        "vector:int",
        "vector:compare",
        "vector:select",
        "vector:shuffle",
        "vector:cast",
        "vector:intrinsic",
        "vector:extract",
        "vector:reduce",
        "vector:bitcast",
        "vector:mask-bitcast",
    ];

    /// The count of `name`; 0 for a name that is not a counter.
    pub fn get(&self, name: &str) -> u64 {
        self.0.get(name).copied().unwrap_or(0)
    }

    /// The counters that are not zero.
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, u64)> + '_ {
        self.0.iter().map(|(&name, &count)| (name, count))
    }

    /// Adds the counts of `other`.
    pub fn merge(&mut self, other: &Coverage) {
        for (name, count) in other.iter() {
            *self.0.entry(name).or_default() += count;
        }
    }

    fn hit(&mut self, name: &'static str) {
        debug_assert!(
            Coverage::NAMES.contains(&name)
                || Coverage::UNWIND_NAMES.contains(&name)
                || Coverage::VECTOR_NAMES.contains(&name),
            "{name}"
        );
        *self.0.entry(name).or_default() += 1;
    }

    /// Counts how the results of `body` are used (the `use:` counters).
    fn scan(&mut self, body: &Body) {
        let mut calls_before = Vec::with_capacity(body.inst_count() + 1);
        let mut calls = 0u32;
        for inst in body.insts() {
            calls_before.push(calls);
            // A call of an intrinsic has no callee.
            let real = inst.opcode() == Opcode::Call
                && inst.operands().first().is_some_and(|callee| !callee.is_none());
            calls += u32::from(real);
        }
        calls_before.push(calls);
        for inst in body.insts() {
            if inst.ty() == TypeId::VOID {
                continue;
            }
            let uses = inst.uses();
            if uses.is_unused() {
                self.hit("use:dead");
            } else if uses.only_use_is_next() {
                self.hit("use:next");
            } else if uses.count() == 1 {
                self.hit("use:once-later");
            } else {
                self.hit("use:many");
            }
            if uses.used_outside_block() {
                self.hit("use:other-block");
            }
        }
        for inst in body.insts() {
            let user = inst.id().index();
            let across = inst.operands().iter().any(|operand| {
                operand.as_inst().is_some_and(|def| {
                    def.index() < user && calls_before[user] > calls_before[def.index() + 1]
                })
            });
            if across {
                self.hit("use:across-call");
            }
        }
    }
}

/// A generated program (§9.1).
#[derive(Debug)]
pub struct Generated {
    /// The module: the functions, the variables and the declarations of
    /// the host functions the program calls.
    pub module: Module,
    /// The body of every function of the module that has one.
    pub bodies: Vec<Body>,
    /// The function to run: `i64 entry(i64, i64)`.
    pub entry: SymId,
    /// The arguments to run it with.
    pub args: Vec<Val>,
    /// A run takes at most this many steps.
    pub fuel_bound: u64,
    /// The standard host functions, when the program may call them.
    pub hosts: Option<Standard>,
    /// What the program contains.
    pub coverage: Coverage,
}

impl Generated {
    /// The program, for [`Interp::new`].
    pub fn program(&self) -> Program<'_> {
        Program::with_bodies(&self.module, &self.bodies)
            .expect("a generated body belongs to its module")
    }

    /// Runs the entry function on the arguments with the standard host
    /// functions, the fuel bound as the fuel, room for a call chain
    /// through every function, and the other limits at their defaults.
    pub fn run(&self) -> Outcome {
        let default = Config::default();
        let functions = u32::try_from(self.bodies.len()).unwrap_or(u32::MAX);
        self.run_with(Config {
            fuel: self.fuel_bound,
            max_call_depth: default.max_call_depth.max(functions),
            ..default
        })
    }

    /// As [`Generated::run`], with `config`.
    pub fn run_with(&self, config: Config) -> Outcome {
        let program = self.program();
        let mut interp = Interp::new(&program).with_config(config);
        host::register(&mut interp);
        interp.run(self.entry, &self.args)
    }
}

/// The program of `seed` under `options`.
///
/// # Panics
/// Only if the generator itself is wrong: it builds nothing `hod_ir`
/// refuses.
pub fn generate(seed: u64, options: &Options) -> Generated {
    match Gen::run(seed, options) {
        Ok(generated) => generated,
        Err(error) => panic!("the generator built something hod_ir refuses (seed {seed}): {error}"),
    }
}

// ---- randomness ----

/// `SplitMix64`.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number below `n`; 0 when `n` is 0.
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize]
    }

    /// An integer of `bits` bits: often one of the values at the edges.
    fn interesting(&mut self, bits: u32) -> u128 {
        let all = mask(bits);
        match self.below(16) {
            0 => 0,
            1 => 1,
            2 => all,
            3 => 1 << (bits - 1),
            4 => all >> 1,
            5 | 6 => u128::from(self.below(17)) & all,
            _ => ((u128::from(self.next()) << 64) | u128::from(self.next())) & all,
        }
    }
}

// ---- what the generator knows about values ----

/// The types of the values the generator computes with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    Int(u32),
    F32,
    F64,
    /// A pointer to the start of a byte region (see [`Elem::Bytes`]) that
    /// may be written.
    Bytes,
}

/// A bound on a float that operations keep (§9.2): its magnitude is below
/// `2^mag`, so nothing overflows to an infinity and no NaN arises.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Fl {
    mag: u8,
    /// Not negative.
    nonneg: bool,
    /// At least 1 in magnitude: a divisor.
    nz: bool,
}

const NO_FL: Fl = Fl { mag: 0, nonneg: false, nz: false };

/// No float is ever at or above `2^MAX_MAG`; `float` holds up to `2^127`.
const MAX_MAG: u8 = 90;
/// What a float in memory, in a parameter, in a return value or carried
/// around a loop is below.
const BOUND: u8 = 30;

/// A value that may be used where the builder is.
#[derive(Clone, Copy, Debug)]
struct Avail {
    value: Value,
    ty: Ty,
    fl: Fl,
    /// To be used once and no more.
    once: bool,
    /// Was used once.
    dead: bool,
}

/// What a region of memory holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Elem {
    /// 16 bytes, aligned to 8, all written: any of `i8`, `i16`, `i32` and
    /// `i64` may be loaded and stored at multiples of its size.
    Bytes,
    /// Cells of one type, each loaded and stored as that type only.
    Cell(Ty),
}

/// Memory the generator may access: `len` elements at `base + offset`.
#[derive(Clone, Copy, Debug)]
struct Arr {
    base: Value,
    offset: i64,
    elem: Elem,
    /// A power of two.
    len: u64,
    align: u64,
    writable: bool,
    global: bool,
}

struct Sig {
    params: Vec<Ty>,
    ret: Option<Ty>,
    fn_ty: TypeId,
    /// The constant table of the functions of this signature.
    table: Option<SymId>,
    /// The functions of this signature, in table order.
    members: Vec<usize>,
}

struct Func {
    sym: SymId,
    sig: usize,
    /// A call takes at most this many steps. Known once generated.
    cost: u64,
    /// The exceptions a call may end in: bit `t` for type
    /// [`eh::TYPE_INFOS`]`[t]`. Known once generated.
    escapes: u8,
}

fn width_key(bits: u32) -> &'static str {
    match bits {
        1 => "int:i1",
        8 => "int:i8",
        16 => "int:i16",
        32 => "int:i32",
        7 => "int:i7",
        33 => "int:i33",
        128 => "int:i128",
        _ => "int:i64",
    }
}

fn aligned(bytes: u64) -> IrResult<MemFlags> {
    MemFlags::aligned(bytes)
}

// ---- the program ----

struct Gen {
    rng: Rng,
    opt: Options,
    m: Module,
    cov: Coverage,
    /// The integer widths in use.
    ints: Vec<u32>,
    sigs: Vec<Sig>,
    fns: Vec<Func>,
    globals: Vec<Arr>,
    std: Option<Standard>,
    eh: Option<Eh>,
}

impl Gen {
    fn run(seed: u64, options: &Options) -> IrResult<Generated> {
        let opt = options.effective();
        let mut ints = vec![64];
        if opt.narrow_ints {
            ints.extend([1, 8, 16, 32]);
        }
        if opt.odd_ints {
            ints.extend([7, 33]);
        }
        if opt.i128 {
            ints.push(128);
        }
        let mut g = Gen {
            rng: Rng(seed),
            m: Module::new(opt.target.clone()),
            opt,
            cov: Coverage::default(),
            ints,
            sigs: Vec::new(),
            fns: Vec::new(),
            globals: Vec::new(),
            std: None,
            eh: None,
        };
        if g.opt.hosts {
            g.std = Some(host::declare(&mut g.m)?);
        }
        if g.opt.exceptions {
            g.eh = Some(eh::declare(&mut g.m)?);
        }
        g.declare_functions()?;
        if g.opt.globals {
            g.make_globals()?;
        }
        if g.opt.indirect_calls && g.opt.globals {
            g.make_tables()?;
        }
        let mut bodies = Vec::new();
        for index in (0..g.fns.len()).rev() {
            let body = FnGen::build(&mut g, index)?;
            body.verify(&g.m)?;
            g.cov.scan(&body);
            bodies.push(body);
        }
        bodies.reverse();
        let mut args = Vec::new();
        for _ in 0..2 {
            let edge = g.rng.chance(25);
            args.push(Val::i64(if edge { g.rng.interesting(64) as u64 } else { g.rng.next() }));
        }
        Ok(Generated {
            entry: g.fns[0].sym,
            fuel_bound: g.fns[0].cost,
            module: g.m,
            bodies,
            args,
            hosts: g.std,
            coverage: g.cov,
        })
    }

    fn tid(&mut self, ty: Ty) -> IrResult<TypeId> {
        match ty {
            Ty::Int(bits) => self.m.types.int(bits),
            Ty::F32 => Ok(TypeId::FLOAT),
            Ty::F64 => Ok(TypeId::DOUBLE),
            Ty::Bytes => Ok(TypeId::PTR),
        }
    }

    /// A type a value may have: an integer in use or a float.
    fn value_ty(&mut self) -> Ty {
        if self.opt.floats && self.rng.chance(15) {
            return self.rng.pick(&[Ty::F32, Ty::F64]);
        }
        Ty::Int(self.rng.pick(&self.ints))
    }

    fn declare_functions(&mut self) -> IrResult<()> {
        let count = self.opt.functions as usize;
        let entry_ty = self.m.types.function(TypeId::I64, &[TypeId::I64, TypeId::I64], false)?;
        self.sigs.push(Sig {
            params: vec![Ty::Int(64), Ty::Int(64)],
            ret: Some(Ty::Int(64)),
            fn_ty: entry_ty,
            table: None,
            members: Vec::new(),
        });
        for _ in 0..count / 2 {
            let many = self.opt.many_args && self.rng.chance(30);
            let arity = if many { 9 + self.rng.below(4) } else { self.rng.below(5) };
            let mut params = Vec::new();
            for _ in 0..arity {
                let pointer = self.opt.memory && self.rng.chance(15);
                params.push(if pointer { Ty::Bytes } else { self.value_ty() });
            }
            let ret = if self.rng.chance(15) { None } else { Some(self.value_ty()) };
            let mut ids = Vec::new();
            for &param in &params {
                ids.push(self.tid(param)?);
            }
            let ret_id = match ret {
                Some(ty) => self.tid(ty)?,
                None => TypeId::VOID,
            };
            let fn_ty = self.m.types.function(ret_id, &ids, false)?;
            self.sigs.push(Sig { params, ret, fn_ty, table: None, members: Vec::new() });
        }
        for index in 0..count {
            let sig = if index == 0 || self.sigs.len() == 1 {
                0
            } else {
                1 + self.rng.below(self.sigs.len() as u64 - 1) as usize
            };
            let name = if index == 0 { "entry".to_owned() } else { format!("f{index}") };
            let personality = self.eh.map(|eh| eh.personality);
            let desc = FunctionDesc {
                has_body: true,
                personality,
                ..FunctionDesc::new(self.sigs[sig].fn_ty)
            };
            let sym = self.m.add_function(name.as_bytes(), &desc)?;
            if index != 0 {
                self.sigs[sig].members.push(index);
            }
            self.fns.push(Func { sym, sig, cost: 0, escapes: 0 });
        }
        Ok(())
    }

    fn cell_const(&mut self, ty: Ty) -> IrResult<hod_ir::ConstId> {
        match ty {
            Ty::Int(bits) => {
                let value = self.rng.interesting(bits);
                let id = self.m.types.int(bits)?;
                self.m.consts.int_from_words(
                    &self.m.types,
                    id,
                    &[value as u64, (value >> 64) as u64],
                )
            }
            Ty::F32 => self.m.consts.f32(eighth(self.rng.below(129)) as f32),
            Ty::F64 => self.m.consts.f64(eighth(self.rng.below(129))),
            Ty::Bytes => self.m.consts.null(&self.m.types, TypeId::PTR),
        }
    }

    /// A variable `name` that is an array of `len` cells of `ty`.
    fn global_cells(&mut self, name: &str, ty: Ty, len: u64, constant: bool) -> IrResult<()> {
        let elem = self.tid(ty)?;
        let array = self.m.types.array(elem, len)?;
        let mut elems = Vec::new();
        for _ in 0..len {
            elems.push(self.cell_const(ty)?);
        }
        let init = self.m.consts.aggregate(&self.m.types, array, &elems)?;
        let align = self.m.types.align_of(elem);
        let desc = VariableDesc {
            init: Some(init),
            constant,
            align: Some(align),
            ..VariableDesc::new(array)
        };
        let sym = self.m.add_variable(name.as_bytes(), &desc)?;
        self.globals.push(Arr {
            base: Value::sym(sym)?,
            offset: 0,
            elem: Elem::Cell(ty),
            len,
            align,
            writable: !constant,
            global: true,
        });
        Ok(())
    }

    fn make_globals(&mut self) -> IrResult<()> {
        // Two byte regions, one of them constant.
        for (name, constant) in [("g_bytes", false), ("g_rodata", true)] {
            let data: Vec<u8> = (0..16).map(|_| self.rng.next() as u8).collect();
            let init = self.m.consts.bytes(&mut self.m.types, &data)?;
            let ty = self.m.consts.ty(init);
            let desc = VariableDesc {
                init: Some(init),
                constant,
                align: Some(8),
                ..VariableDesc::new(ty)
            };
            let sym = self.m.add_variable(name.as_bytes(), &desc)?;
            self.globals.push(Arr {
                base: Value::sym(sym)?,
                offset: 0,
                elem: Elem::Bytes,
                len: 16,
                align: 8,
                writable: !constant,
                global: true,
            });
        }
        self.global_cells("g_word", Ty::Int(64), 1, false)?;
        self.global_cells("g_table", Ty::Int(64), 4, true)?;
        if self.opt.narrow_ints {
            self.global_cells("g_ints", Ty::Int(32), 4, false)?;
            // A struct with padding after its first and third fields.
            let fields = [Ty::Int(8), Ty::Int(32), Ty::Int(16), Ty::Int(64)];
            let ids = [TypeId::I8, TypeId::I32, TypeId::I16, TypeId::I64];
            let ty = self.m.types.struct_type(&ids, false)?;
            let mut elems = Vec::new();
            for field in fields {
                elems.push(self.cell_const(field)?);
            }
            let init = self.m.consts.aggregate(&self.m.types, ty, &elems)?;
            let desc = VariableDesc { init: Some(init), ..VariableDesc::new(ty) };
            let sym = self.m.add_variable(b"g_struct", &desc)?;
            for (index, (field, id)) in fields.into_iter().zip(ids).enumerate() {
                self.globals.push(Arr {
                    base: Value::sym(sym)?,
                    offset: self.m.types.field_offset(ty, index as u32) as i64,
                    elem: Elem::Cell(field),
                    len: 1,
                    align: self.m.types.align_of(id),
                    writable: true,
                    global: true,
                });
            }
        }
        if self.opt.odd_ints {
            self.global_cells("g_odd", Ty::Int(33), 2, false)?;
        }
        if self.opt.floats {
            self.global_cells("g_doubles", Ty::F64, 2, false)?;
        }
        Ok(())
    }

    /// One constant array of function addresses for each signature.
    fn make_tables(&mut self) -> IrResult<()> {
        for index in 1..self.sigs.len() {
            let members = self.sigs[index].members.clone();
            if members.is_empty() {
                continue;
            }
            let ty = self.m.types.array(TypeId::PTR, members.len() as u64)?;
            let mut elems = Vec::new();
            for &member in &members {
                elems.push(self.m.consts.addr(self.fns[member].sym, 0)?);
            }
            let init = self.m.consts.aggregate(&self.m.types, ty, &elems)?;
            let desc = VariableDesc {
                init: Some(init),
                constant: true,
                align: Some(8),
                ..VariableDesc::new(ty)
            };
            let name = format!("g_fns{index}");
            self.sigs[index].table = Some(self.m.add_variable(name.as_bytes(), &desc)?);
        }
        Ok(())
    }
}

// ---- one function ----

/// The deepest nesting of control-flow constructs.
const MAX_DEPTH: u32 = 3;
/// The most iterations of a generated loop.
const MAX_TRIP: u64 = 4;

struct FnGen<'g> {
    g: &'g mut Gen,
    b: FunctionBuilder,
    index: usize,
    pool: Vec<Avail>,
    arrs: Vec<Arr>,
    /// How often the code being built runs per call, at most.
    mult: u64,
    /// Steps of the instructions charged so far.
    cost: u64,
    /// Instructions charged so far.
    charged: usize,
    /// The exceptions that may leave the function (see [`Func::escapes`]).
    escapes: u8,
    /// How many landing pads the builder is in.
    pads: u32,
    /// Computed gotos so far, for the names of their tables.
    gotos: u32,
}

impl<'g> FnGen<'g> {
    fn build(g: &'g mut Gen, index: usize) -> IrResult<Body> {
        let (sym, sig) = (g.fns[index].sym, g.fns[index].sig);
        let b = FunctionBuilder::new(&g.m, sym, g.sigs[sig].fn_ty)?;
        let arrs = g.globals.clone();
        let mut f = FnGen {
            g,
            b,
            index,
            pool: Vec::new(),
            arrs,
            mult: 1,
            cost: 0,
            charged: 0,
            escapes: 0,
            pads: 0,
            gotos: 0,
        };
        let entry = f.b.create_block()?;
        f.b.begin_block(entry)?;
        let params = f.g.sigs[sig].params.clone();
        for (at, &ty) in params.iter().enumerate() {
            let value = f.b.arg(at as u32)?;
            if ty == Ty::Bytes {
                f.arrs.push(Arr {
                    base: value,
                    offset: 0,
                    elem: Elem::Bytes,
                    len: 16,
                    align: 8,
                    writable: true,
                    global: false,
                });
            } else {
                f.push_fl(value, ty, Fl { mag: BOUND, nonneg: false, nz: false });
            }
        }
        if f.g.opt.memory {
            f.make_slots()?;
        }
        // A call in the middle, where it is always reached, so that the
        // other functions run more often than not.
        let budget = f.g.opt.insts as usize;
        f.region(budget / 2, 0)?;
        if f.g.opt.calls {
            f.call()?;
        }
        f.region(budget - budget / 2, 0)?;
        f.finish(sig)?;
        f.charge();
        f.g.fns[index].cost = f.cost;
        debug_assert!(index != 0 || f.escapes == 0, "an exception may leave the entry function");
        f.g.fns[index].escapes = f.escapes;
        f.b.finish()
    }

    // ---- small helpers ----

    fn rng(&mut self) -> &mut Rng {
        &mut self.g.rng
    }

    /// One of the integer widths in use.
    fn int_width(&mut self) -> u32 {
        let g = &mut *self.g;
        g.rng.pick(&g.ints)
    }

    fn hit(&mut self, name: &'static str) {
        self.g.cov.hit(name);
    }

    fn tid(&mut self, ty: Ty) -> IrResult<TypeId> {
        self.g.tid(ty)
    }

    /// Counts the instructions built since the last call at the current
    /// multiplicity.
    fn charge(&mut self) {
        let now = self.b.inst_count();
        self.cost += (now - self.charged) as u64 * self.mult;
        self.charged = now;
    }

    fn cint(&mut self, bits: u32, value: u128) -> IrResult<Value> {
        let value = value & mask(bits);
        let m = &mut self.g.m;
        let ty = m.types.int(bits)?;
        Value::constant(m.consts.int_from_words(
            &m.types,
            ty,
            &[value as u64, (value >> 64) as u64],
        )?)
    }

    fn push(&mut self, value: Value, ty: Ty) {
        self.push_fl(value, ty, NO_FL);
    }

    fn push_fl(&mut self, value: Value, ty: Ty, fl: Fl) {
        self.pool.push(Avail { value, ty, fl, once: false, dead: false });
    }

    /// Takes pool entry `at`: a value to be used once is used up.
    fn take(&mut self, at: usize) -> Avail {
        let avail = self.pool[at];
        if avail.once {
            self.pool[at].dead = true;
        }
        avail
    }

    /// A random live pool entry, from the newest ones when `recent`.
    fn probe(&mut self, recent: bool) -> Option<usize> {
        let len = self.pool.len() as u64;
        if len == 0 {
            return None;
        }
        let back = if recent { self.rng().below(len.min(12)) } else { self.rng().below(len) };
        let at = (len - 1 - back) as usize;
        (!self.pool[at].dead).then_some(at)
    }

    fn bin(&mut self, op: Opcode, bits: u32, lhs: Value, rhs: Value) -> IrResult<Value> {
        self.hit(width_key(bits));
        let ty = self.tid(Ty::Int(bits))?;
        self.b.binary(op, ty, lhs, rhs)
    }

    fn cast(&mut self, op: Opcode, to: Ty, arg: Value) -> IrResult<Value> {
        match op {
            Opcode::Trunc => self.hit("cast:trunc"),
            Opcode::ZExt => self.hit("cast:zext"),
            Opcode::SExt => self.hit("cast:sext"),
            Opcode::PtrToInt => self.hit("cast:ptrtoint"),
            Opcode::IntToPtr => self.hit("cast:inttoptr"),
            _ => self.hit("float:convert"),
        }
        let ty = self.tid(to)?;
        self.b.cast(op, ty, arg)
    }

    /// `value`, an integer of `from` bits, as one of `to` bits.
    fn resize(&mut self, value: Value, from: u32, to: u32) -> IrResult<Value> {
        if from == to {
            return Ok(value);
        }
        let op = if from > to {
            Opcode::Trunc
        } else if self.rng().chance(50) {
            Opcode::ZExt
        } else {
            Opcode::SExt
        };
        self.cast(op, Ty::Int(to), value)
    }

    // ---- operands ----

    /// An integer of `bits` bits: from the pool, converted from another
    /// width, or a constant.
    fn pick_int(&mut self, bits: u32) -> IrResult<Value> {
        let want = Ty::Int(bits);
        let mut other = None;
        for attempt in 0..8 {
            let Some(at) = self.probe(attempt < 4) else { continue };
            let found = self.pool[at].ty;
            if found == want {
                return Ok(self.take(at).value);
            }
            if matches!(found, Ty::Int(_)) && other.is_none() {
                other = Some(at);
            }
        }
        if let Some(at) = other
            && self.g.opt.casts
            && self.rng().chance(70)
        {
            let avail = self.take(at);
            let Ty::Int(from) = avail.ty else { return Ok(avail.value) };
            let value = self.resize(avail.value, from, bits)?;
            self.push(value, want);
            return Ok(value);
        }
        let value = self.rng().interesting(bits);
        self.cint(bits, value)
    }

    /// A float of type `ty` below `2^max_mag`, a divisor if `nz`, not
    /// negative if `nonneg`: from the pool, or made from an integer.
    fn pick_float(&mut self, ty: Ty, max_mag: u8, nz: bool, nonneg: bool) -> IrResult<Avail> {
        for attempt in 0..8 {
            let Some(at) = self.probe(attempt < 4) else { continue };
            let found = self.pool[at];
            if found.ty == ty
                && found.fl.mag <= max_mag
                && (!nz || found.fl.nz)
                && (!nonneg || found.fl.nonneg)
            {
                return Ok(self.take(at));
            }
        }
        let (value, fl) = if !nz && self.rng().chance(25) {
            // An eighth between -8 and 8.
            let low = if nonneg { 64 } else { 0 };
            let eighths = low + self.rng().below(129 - low);
            let x = eighth(eighths);
            let id = match ty {
                Ty::F32 => self.g.m.consts.f32(x as f32)?,
                _ => self.g.m.consts.f64(x)?,
            };
            (Value::constant(id)?, Fl { mag: 4, nonneg: x >= 0.0, nz: x.abs() >= 1.0 })
        } else {
            // An integer of twelve bits, made odd for a divisor.
            let wide = self.pick_int(64)?;
            let low = self.cint(64, 0xfff)?;
            let mut small = self.bin(Opcode::And, 64, wide, low)?;
            if nz {
                let one = self.cint(64, 1)?;
                small = self.bin(Opcode::Or, 64, small, one)?;
            }
            if !nz && !nonneg && self.rng().chance(50) {
                let half = self.cint(64, 2048)?;
                let centred = self.bin(Opcode::Sub, 64, small, half)?;
                let value = self.cast(Opcode::SIToFP, ty, centred)?;
                (value, Fl { mag: 12, nonneg: false, nz: false })
            } else {
                let value = self.cast(Opcode::UIToFP, ty, small)?;
                (value, Fl { mag: 12, nonneg: true, nz })
            }
        };
        self.push_fl(value, ty, fl);
        Ok(Avail { value, ty, fl, once: false, dead: false })
    }

    /// The start of a byte region.
    fn pick_bytes(&mut self, writable: bool) -> Value {
        for _ in 0..4 {
            if let Some(at) = self.probe(false)
                && self.pool[at].ty == Ty::Bytes
            {
                return self.take(at).value;
            }
        }
        let regions: Vec<Value> = self
            .arrs
            .iter()
            .filter(|arr| arr.elem == Elem::Bytes && arr.offset == 0 && (arr.writable || !writable))
            .map(|arr| arr.base)
            .collect();
        // Every function with `memory` on has a byte slot of its own.
        self.rng().pick(&regions)
    }

    /// A value of type `ty` with its bounds.
    fn pick(&mut self, ty: Ty, max_mag: u8) -> IrResult<Avail> {
        Ok(match ty {
            Ty::Int(bits) => {
                let value = self.pick_int(bits)?;
                Avail { value, ty, fl: NO_FL, once: false, dead: false }
            }
            Ty::F32 | Ty::F64 => self.pick_float(ty, max_mag, false, false)?,
            Ty::Bytes => {
                let value = self.pick_bytes(true);
                Avail { value, ty, fl: NO_FL, once: false, dead: false }
            }
        })
    }

    /// A type for a `phi`, a `select` or a slot.
    fn merge_ty(&mut self) -> Ty {
        if self.g.opt.memory && self.rng().chance(8) {
            return Ty::Bytes;
        }
        self.g.value_ty()
    }

    /// A condition: an `i1` from the pool or a new compare.
    fn cond(&mut self) -> IrResult<Value> {
        if self.rng().chance(30) {
            for _ in 0..4 {
                if let Some(at) = self.probe(true)
                    && self.pool[at].ty == Ty::Int(1)
                {
                    return Ok(self.take(at).value);
                }
            }
        }
        self.hit("icmp:branch");
        self.icmp()
    }

    fn icmp(&mut self) -> IrResult<Value> {
        let bits = self.int_width();
        let (lhs, rhs) = (self.pick_int(bits)?, self.pick_int(bits)?);
        let pred = self.rng().pick(IntPredicate::ALL);
        self.b.icmp(pred, TypeId::I1, lhs, rhs)
    }

    /// Any pool value as an `i64` that depends on all of it.
    fn observe(&mut self) -> IrResult<Value> {
        let found = (0..4).find_map(|_| self.probe(false));
        let Some(at) = found else {
            let value = self.rng().interesting(64);
            return self.cint(64, value);
        };
        let avail = self.take(at);
        match avail.ty {
            Ty::Int(64) => Ok(avail.value),
            Ty::Int(128) => {
                let by = self.cint(128, 64)?;
                let high = self.bin(Opcode::LShr, 128, avail.value, by)?;
                let mixed = self.bin(Opcode::Xor, 128, avail.value, high)?;
                self.cast(Opcode::Trunc, Ty::Int(64), mixed)
            }
            Ty::Int(bits) if self.g.opt.casts => self.resize(avail.value, bits, 64),
            Ty::Int(_) => self.cint(64, 1),
            Ty::F64 => self.cast(Opcode::BitCast, Ty::Int(64), avail.value),
            Ty::F32 => {
                let wide = self.cast(Opcode::FPExt, Ty::F64, avail.value)?;
                self.cast(Opcode::BitCast, Ty::Int(64), wide)
            }
            Ty::Bytes => {
                self.hit("mem:load");
                self.b.load(TypeId::I64, avail.value, aligned(8)?)
            }
        }
    }

    // ---- straight-line operations ----

    /// Puts a new integer into the pool in one of the shapes of §9.3:
    /// never used, used by the next instruction only, used once later,
    /// or used any number of times.
    fn finish_int(&mut self, value: Value, bits: u32) -> IrResult<()> {
        match self.rng().below(100) {
            0..8 => {}
            8..20 => {
                let next = self.int_binary(Some(value), bits)?;
                self.push(next, Ty::Int(bits));
            }
            20..32 => {
                self.pool.push(Avail {
                    value,
                    ty: Ty::Int(bits),
                    fl: NO_FL,
                    once: true,
                    dead: false,
                });
            }
            _ => self.push(value, Ty::Int(bits)),
        }
        Ok(())
    }

    /// An integer operation whose operands cannot make it trap (§9.2).
    fn int_binary(&mut self, lhs: Option<Value>, bits: u32) -> IrResult<Value> {
        use Opcode::{AShr, Add, And, LShr, Mul, Or, SDiv, SRem, Shl, Sub, UDiv, URem, Xor};
        // More of what keeps every bit of its operands in play.
        let mut op = self.rng().pick(&[
            Add, Add, Add, Sub, Sub, Sub, Xor, Xor, Xor, Mul, Mul, And, Or, Shl, LShr, AShr, UDiv,
            SDiv, URem, SRem,
        ]);
        if bits == 1 && matches!(op, SDiv | SRem) {
            // The only divisor that is not zero is -1.
            op = UDiv;
        }
        let lhs = match lhs {
            Some(lhs) => lhs,
            None => self.pick_int(bits)?,
        };
        let constant = self.rng().chance(30);
        // Mostly small divisors and short shifts: a quotient by a large
        // divisor is 0 or 1, and a long shift leaves little.
        let small = self.rng().chance(70);
        let positive = mask(bits) >> 1;
        let rhs = match op {
            UDiv | URem if constant => {
                let range = if small { mask(bits).min(255) } else { mask(bits) };
                let divisor = 1 + u128::from(self.rng().next()) % range;
                self.cint(bits, divisor.min(mask(bits)))?
            }
            UDiv | URem => {
                let mut any = self.pick_int(bits)?;
                if small && bits > 8 {
                    let low = self.cint(bits, 255)?;
                    any = self.bin(And, bits, any, low)?;
                }
                let one = self.cint(bits, 1)?;
                self.bin(Or, bits, any, one)?
            }
            // A positive divisor: not zero and not -1.
            SDiv | SRem if constant => {
                let range = if small { positive.min(255) } else { positive };
                let divisor = 1 + u128::from(self.rng().next()) % range;
                self.cint(bits, divisor.min(positive))?
            }
            SDiv | SRem => {
                let any = self.pick_int(bits)?;
                let keep = if small { positive.min(255) } else { positive };
                let keep = self.cint(bits, keep)?;
                let cut = self.bin(And, bits, any, keep)?;
                let one = self.cint(bits, 1)?;
                self.bin(Or, bits, cut, one)?
            }
            Shl | LShr | AShr if constant => {
                let below = if small { bits.min(8) } else { bits };
                let by = self.rng().below(u64::from(below));
                self.cint(bits, u128::from(by))?
            }
            Shl | LShr | AShr => {
                // The largest `2^k - 1` below the width.
                let limit = (1u128 << bits.ilog2()) - 1;
                let limit = if small { limit.min(7) } else { limit };
                let any = self.pick_int(bits)?;
                let limit = self.cint(bits, limit)?;
                self.bin(And, bits, any, limit)?
            }
            // By an odd number nothing is lost.
            Mul if constant || small => {
                let odd = self.rng().next() | 1;
                self.cint(bits, u128::from(odd))?
            }
            _ => self.pick_int(bits)?,
        };
        self.bin(op, bits, lhs, rhs)
    }

    fn float_op(&mut self) -> IrResult<()> {
        let ty = self.rng().pick(&[Ty::F32, Ty::F64]);
        let id = self.tid(ty)?;
        match self.rng().below(12) {
            0..3 => {
                let op = self.rng().pick(&[Opcode::FAdd, Opcode::FSub]);
                let a = self.pick_float(ty, MAX_MAG - 1, false, false)?;
                let b = self.pick_float(ty, MAX_MAG - 1, false, false)?;
                let value = self.b.binary(op, id, a.value, b.value)?;
                let nonneg = op == Opcode::FAdd && a.fl.nonneg && b.fl.nonneg;
                let fl = Fl { mag: a.fl.mag.max(b.fl.mag) + 1, nonneg, nz: false };
                self.hit("float:op");
                self.push_fl(value, ty, fl);
            }
            3..5 => {
                let a = self.pick_float(ty, MAX_MAG / 2, false, false)?;
                let b = self.pick_float(ty, MAX_MAG / 2, false, false)?;
                let value = self.b.binary(Opcode::FMul, id, a.value, b.value)?;
                let nonneg = a.fl.nonneg && b.fl.nonneg;
                let fl = Fl { mag: a.fl.mag + b.fl.mag, nonneg, nz: a.fl.nz && b.fl.nz };
                self.hit("float:op");
                self.push_fl(value, ty, fl);
            }
            5..7 => {
                // The divisor is at least 1 in magnitude: the quotient is
                // no larger than the dividend, the remainder smaller.
                let op = self.rng().pick(&[Opcode::FDiv, Opcode::FRem]);
                let a = self.pick_float(ty, MAX_MAG, false, false)?;
                let b = self.pick_float(ty, MAX_MAG, true, false)?;
                let value = self.b.binary(op, id, a.value, b.value)?;
                let nonneg = a.fl.nonneg && (b.fl.nonneg || op == Opcode::FRem);
                self.hit("float:op");
                self.push_fl(value, ty, Fl { mag: a.fl.mag, nonneg, nz: false });
            }
            7 => {
                let a = self.pick_float(ty, MAX_MAG, false, false)?;
                let value = self.b.fneg(id, a.value)?;
                self.hit("float:op");
                self.push_fl(value, ty, Fl { nonneg: false, ..a.fl });
            }
            8 => {
                let a = self.pick_float(ty, MAX_MAG, false, false)?;
                let b = self.pick_float(ty, MAX_MAG, false, false)?;
                let pred = self.rng().pick(FloatPredicate::ALL);
                let value = self.b.fcmp(pred, TypeId::I1, a.value, b.value)?;
                self.hit("float:fcmp");
                self.push(value, Ty::Int(1));
            }
            9 => {
                // To an integer the value fits: `i64` holds what is below
                // `2^62` either way.
                let a = self.pick_float(ty, 62, false, false)?;
                let op = if a.fl.nonneg && self.rng().chance(50) {
                    Opcode::FPToUI
                } else {
                    Opcode::FPToSI
                };
                let value = self.cast(op, Ty::Int(64), a.value)?;
                self.push(value, Ty::Int(64));
            }
            10 => {
                let a = self.pick_float(ty, MAX_MAG - 1, false, false)?;
                let (op, to, mag) = match ty {
                    Ty::F32 => (Opcode::FPExt, Ty::F64, a.fl.mag),
                    // Rounding may reach the next power of two.
                    _ => (Opcode::FPTrunc, Ty::F32, a.fl.mag + 1),
                };
                let value = self.cast(op, to, a.value)?;
                self.push_fl(value, to, Fl { mag, ..a.fl });
            }
            _ => {
                let widths: Vec<u32> =
                    self.g.ints.iter().copied().filter(|&bits| bits <= 64).collect();
                let bits = self.rng().pick(&widths);
                let from = self.pick_int(bits)?;
                let signed = self.rng().chance(50);
                let op = if signed { Opcode::SIToFP } else { Opcode::UIToFP };
                let value = self.cast(op, ty, from)?;
                // An unsigned one may round up to `2^bits` itself.
                let fl = Fl { mag: bits as u8 + 1, nonneg: !signed, nz: false };
                self.push_fl(value, ty, fl);
            }
        }
        Ok(())
    }

    /// The address of element `index` of `arr`, of `size` bytes each,
    /// where `count` elements fit: by a constant offset or a scaled index.
    fn element(&mut self, arr: Arr, size: u64, count: u64) -> IrResult<Value> {
        if count > 1 && self.rng().chance(50) {
            // An index of any width, kept below the count.
            let widths: Vec<u32> =
                self.g.ints.iter().copied().filter(|&bits| bits > 4 && bits <= 64).collect();
            let bits = self.rng().pick(&widths);
            let any = self.pick_int(bits)?;
            // With a constant part, the index covers the rest.
            let fixed = if self.rng().chance(30) { count / 2 } else { 0 };
            let limit = self.cint(bits, u128::from(count - fixed - 1))?;
            let index = self.bin(Opcode::And, bits, any, limit)?;
            self.hit("ptradd:scaled");
            let offset = arr.offset + (fixed * size) as i64;
            return self.b.ptr_add(TypeId::PTR, arr.base, offset, &[(index, size)]);
        }
        let offset = arr.offset + (self.rng().below(count) * size) as i64;
        if offset == 0 && self.rng().chance(50) {
            return Ok(arr.base);
        }
        self.hit("ptradd:const");
        if self.rng().chance(15) {
            // There and back: out of the region and in again.
            let far = self.b.ptr_add(TypeId::PTR, arr.base, offset + 4096, &[])?;
            return self.b.ptr_add(TypeId::PTR, far, -4096, &[]);
        }
        self.b.ptr_add(TypeId::PTR, arr.base, offset, &[])
    }

    fn mem_op(&mut self) -> IrResult<()> {
        let count = self.arrs.len() as u64;
        let at = self.rng().below(count) as usize;
        let arr = self.arrs[at];
        let (ty, size, count, natural) = match arr.elem {
            Elem::Bytes => {
                let widths: Vec<u32> =
                    [8, 16, 32, 64].into_iter().filter(|bits| self.g.ints.contains(bits)).collect();
                let bits = self.rng().pick(&widths);
                let size = u64::from(bits / 8);
                (Ty::Int(bits), size, 16 / size, size)
            }
            Elem::Cell(ty) => {
                let id = self.tid(ty)?;
                let types = &self.g.m.types;
                (ty, types.alloc_size_of(id), arr.len, types.align_of(id))
            }
        };
        let id = self.tid(ty)?;
        let addr = self.element(arr, size, count)?;
        let align = if self.rng().chance(20) { 1 } else { natural.min(arr.align) };
        if matches!(ty, Ty::Int(7 | 33)) {
            self.hit("mem:odd-width");
        }
        if arr.writable && self.rng().chance(45) {
            let value = self.pick(ty, BOUND)?.value;
            self.b.store(value, addr, aligned(align)?)?;
            self.hit(if arr.global { "global:store" } else { "mem:store" });
        } else {
            let value = self.b.load(id, addr, aligned(align)?)?;
            self.hit(if arr.global { "global:load" } else { "mem:load" });
            match ty {
                Ty::Int(bits) => self.finish_int(value, bits)?,
                _ => self.push_fl(value, ty, Fl { mag: BOUND, nonneg: false, nz: false }),
            }
        }
        Ok(())
    }

    /// Addresses as integers, in the ways whose results do not depend on
    /// where anything is (§9.2).
    fn ptr_int(&mut self) -> IrResult<()> {
        let base = self.pick_bytes(false);
        match self.rng().below(3) {
            0 => {
                // Through an integer and back, then a load.
                let addr = self.cast(Opcode::PtrToInt, Ty::Int(64), base)?;
                let step = self.rng().below(2) * 8;
                let step = self.cint(64, u128::from(step))?;
                let moved = self.bin(Opcode::Add, 64, addr, step)?;
                let back = self.cast(Opcode::IntToPtr, Ty::Bytes, moved)?;
                let value = self.b.load(TypeId::I64, back, aligned(8)?)?;
                self.hit("mem:load");
                self.push(value, Ty::Int(64));
            }
            1 => {
                // The distance between two addresses in one region.
                let far = 1 + self.rng().below(16);
                let inside = self.b.ptr_add(TypeId::PTR, base, far as i64, &[])?;
                let (high, low) = (
                    self.cast(Opcode::PtrToInt, Ty::Int(64), inside)?,
                    self.cast(Opcode::PtrToInt, Ty::Int(64), base)?,
                );
                let distance = self.bin(Opcode::Sub, 64, high, low)?;
                self.push(distance, Ty::Int(64));
            }
            _ => {
                // The order of two addresses in one region.
                let (first, second) = (self.rng().below(17), self.rng().below(17));
                let (left, right) = (
                    self.b.ptr_add(TypeId::PTR, base, first as i64, &[])?,
                    self.b.ptr_add(TypeId::PTR, base, second as i64, &[])?,
                );
                let pred = self.rng().pick(IntPredicate::ALL);
                // Signed order is the same while both are one side of the
                // sign change, which a region of 16 bytes is.
                let pred = match pred {
                    IntPredicate::Slt => IntPredicate::Ult,
                    IntPredicate::Sle => IntPredicate::Ule,
                    IntPredicate::Sgt => IntPredicate::Ugt,
                    IntPredicate::Sge => IntPredicate::Uge,
                    other => other,
                };
                let value = self.b.icmp(pred, TypeId::I1, left, right)?;
                self.push(value, Ty::Int(1));
            }
        }
        Ok(())
    }

    /// A call of the intrinsic `name`, which is `ret name(params)`.
    fn intrinsic_call(
        &mut self,
        name: &str,
        ret: TypeId,
        params: &[TypeId],
        args: &[Value],
    ) -> IrResult<Value> {
        let fn_ty = self.g.m.types.function(ret, params, false)?;
        let found = resolve_intrinsic(&self.g.m.types, name, fn_ty);
        self.b.call(ret, &CallDesc::intrinsic(found.intrinsic, found.overloads(), fn_ty, args))
    }

    fn intrinsic(&mut self) -> IrResult<()> {
        if self.g.opt.memory && self.rng().chance(20) {
            return self.mem_intrinsic();
        }
        let widths: Vec<u32> =
            [8, 16, 32, 64].into_iter().filter(|bits| self.g.ints.contains(bits)).collect();
        let bits = self.rng().pick(&widths);
        let ty = self.tid(Ty::Int(bits))?;
        let (a, b) = (self.pick_int(bits)?, self.pick_int(bits)?);
        let no = self.cint(1, 0)?;
        let value = match self.rng().below(8) {
            0 => {
                let name = self.rng().pick(&["ctpop", "bitreverse"]);
                self.intrinsic_call(&format!("llvm.{name}.i{bits}"), ty, &[ty], &[a])?
            }
            1 if bits >= 16 => {
                self.intrinsic_call(&format!("llvm.bswap.i{bits}"), ty, &[ty], &[a])?
            }
            1 | 2 => {
                // With the flag off, zero and the smallest value are
                // defined.
                let name = self.rng().pick(&["ctlz", "cttz", "abs"]);
                let params = [ty, TypeId::I1];
                self.intrinsic_call(&format!("llvm.{name}.i{bits}"), ty, &params, &[a, no])?
            }
            3 => {
                let name = self.rng().pick(&["smax", "smin", "umax", "umin"]);
                self.intrinsic_call(&format!("llvm.{name}.i{bits}"), ty, &[ty, ty], &[a, b])?
            }
            4 => {
                let name = self.rng().pick(&["sadd.sat", "uadd.sat", "ssub.sat", "usub.sat"]);
                self.intrinsic_call(&format!("llvm.{name}.i{bits}"), ty, &[ty, ty], &[a, b])?
            }
            5 => {
                // Any amount: it is taken modulo the width.
                let c = self.pick_int(bits)?;
                let name = self.rng().pick(&["fshl", "fshr"]);
                self.intrinsic_call(&format!("llvm.{name}.i{bits}"), ty, &[ty; 3], &[a, b, c])?
            }
            _ => {
                let name = self.rng().pick(&["sadd", "uadd", "ssub", "usub", "smul", "umul"]);
                let pair = self.g.m.types.struct_type(&[ty, TypeId::I1], false)?;
                let flag_at = self.g.m.types.field_offset(pair, 1);
                let name = format!("llvm.{name}.with.overflow.i{bits}");
                let both = self.intrinsic_call(&name, pair, &[ty, ty], &[a, b])?;
                let flag = self.b.extract_value(TypeId::I1, both, flag_at)?;
                self.push(flag, Ty::Int(1));
                self.hit("intrinsic:checked");
                self.b.extract_value(ty, both, 0)?
            }
        };
        self.hit("intrinsic:int");
        self.finish_int(value, bits)
    }

    /// `memset`, `memcpy` and `memmove` on byte regions. Two regions are
    /// the same or apart, so a copy between the same offsets of two of
    /// them is one `memcpy` allows.
    fn mem_intrinsic(&mut self) -> IrResult<()> {
        let (ptr, void, i64t) = (TypeId::PTR, TypeId::VOID, TypeId::I64);
        let dst = self.pick_bytes(true);
        let no = self.cint(1, 0)?;
        let len = self.rng().pick(&[0u64, 1, 4, 8]);
        let at = self.rng().below(9 - len.max(1)) as i64;
        let count = self.cint(64, u128::from(len))?;
        self.hit("intrinsic:mem");
        match self.rng().below(3) {
            0 => {
                let to = self.b.ptr_add(ptr, dst, at, &[])?;
                let byte = if self.g.ints.contains(&8) {
                    self.pick_int(8)?
                } else {
                    let byte = self.rng().below(256);
                    self.cint(8, u128::from(byte))?
                };
                let params = [ptr, TypeId::I8, i64t, TypeId::I1];
                self.intrinsic_call("llvm.memset.p0.i64", void, &params, &[to, byte, count, no])?;
            }
            1 => {
                let src = self.pick_bytes(false);
                let (to, from) =
                    (self.b.ptr_add(ptr, dst, at, &[])?, self.b.ptr_add(ptr, src, at, &[])?);
                let params = [ptr, ptr, i64t, TypeId::I1];
                let args = [to, from, count, no];
                self.intrinsic_call("llvm.memcpy.p0.p0.i64", void, &params, &args)?;
            }
            _ => {
                let src = self.pick_bytes(false);
                let other = self.rng().below(9 - len.max(1)) as i64;
                let (to, from) =
                    (self.b.ptr_add(ptr, dst, at, &[])?, self.b.ptr_add(ptr, src, other, &[])?);
                let params = [ptr, ptr, i64t, TypeId::I1];
                let args = [to, from, count, no];
                self.intrinsic_call("llvm.memmove.p0.p0.i64", void, &params, &args)?;
            }
        }
        Ok(())
    }

    fn select(&mut self) -> IrResult<()> {
        let ty = self.merge_ty();
        let id = self.tid(ty)?;
        let cond = self.cond()?;
        let (a, b) = (self.pick(ty, MAX_MAG)?, self.pick(ty, MAX_MAG)?);
        let value = self.b.select(id, cond, a.value, b.value)?;
        self.hit("select");
        self.push_fl(value, ty, merge_fl(a.fl, b.fl));
        Ok(())
    }

    /// A call of `sink(value)`, if the host functions are on.
    fn sink(&mut self, value: Value) -> IrResult<()> {
        let Some(std) = self.g.std else { return Ok(()) };
        let args = [value];
        self.b.call(TypeId::VOID, &CallDesc::new(Value::sym(std.sink)?, std.sink_ty, &args))?;
        self.hit("call:sink");
        Ok(())
    }

    fn host_call(&mut self) -> IrResult<()> {
        let Some(std) = self.g.std else { return Ok(()) };
        if self.rng().chance(30) {
            let desc = CallDesc::new(Value::sym(std.source)?, std.source_ty, &[]);
            let value = self.b.call(TypeId::I64, &desc)?;
            self.hit("call:source");
            self.finish_int(value, 64)
        } else {
            let value = self.observe()?;
            let args = [value];
            self.b.call(TypeId::VOID, &CallDesc::new(Value::sym(std.sink)?, std.sink_ty, &args))?;
            self.hit("call:sink");
            Ok(())
        }
    }

    /// A call of a function generated earlier, if one fits the budget.
    fn call(&mut self) -> IrResult<()> {
        let count = self.g.fns.len();
        if self.index + 1 >= count {
            return Ok(());
        }
        let later = (count - self.index - 1) as u64;
        let callee = self.index + 1 + self.rng().below(later) as usize;
        let sig = self.g.fns[callee].sig;
        // The functions a call through a pointer may reach.
        let mates: Vec<usize> = self.g.sigs[sig]
            .members
            .iter()
            .copied()
            .filter(|&member| member > self.index)
            .collect();
        let indirect = self.g.opt.indirect_calls && self.rng().chance(40);
        let other = if indirect { self.rng().pick(&mates) } else { callee };
        let callee_cost = self.g.fns[callee].cost.max(self.g.fns[other].cost);
        self.charge();
        if self.cost + self.mult * (callee_cost + 16) > self.g.opt.max_fuel {
            return Ok(());
        }
        let params = self.g.sigs[sig].params.clone();
        let (fn_ty, ret) = (self.g.sigs[sig].fn_ty, self.g.sigs[sig].ret);
        let (sym, other_sym) = (self.g.fns[callee].sym, self.g.fns[other].sym);
        let target = if !indirect {
            self.hit("call:direct");
            Value::sym(sym)?
        } else if let Some(table) = self.g.sigs[sig].table
            && (!self.g.opt.selects || self.rng().chance(50))
        {
            // From the table, at one of two places.
            let place = |member: usize| mates_place(&self.g.sigs[sig].members, member);
            let (first, second) = (place(callee), place(other));
            let index = if self.g.opt.selects {
                let (first, second) = (self.cint(64, first)?, self.cint(64, second)?);
                let which = self.cond()?;
                self.hit("select");
                self.b.select(TypeId::I64, which, first, second)?
            } else {
                self.cint(64, first)?
            };
            let at = self.b.ptr_add(TypeId::PTR, Value::sym(table)?, 0, &[(index, 8)])?;
            self.hit("call:indirect");
            self.hit("ptradd:scaled");
            self.hit("global:load");
            self.b.load(TypeId::PTR, at, aligned(8)?)?
        } else {
            let which = self.cond()?;
            self.hit("call:indirect");
            self.hit("select");
            self.b.select(TypeId::PTR, which, Value::sym(sym)?, Value::sym(other_sym)?)?
        };
        let mut args = Vec::with_capacity(params.len());
        for &param in &params {
            args.push(self.pick(param, BOUND)?.value);
        }
        if params.len() > 8 {
            self.hit("call:many-args");
        }
        let ret_id = match ret {
            Some(ty) => self.tid(ty)?,
            None => TypeId::VOID,
        };
        let desc = CallDesc::new(target, fn_ty, &args);
        let value = if self.g.eh.is_some() {
            let escapes = self.g.fns[callee].escapes | self.g.fns[other].escapes;
            match self.eh_call(&desc, ret, ret_id, escapes)? {
                Some(value) => value,
                None => return Ok(()),
            }
        } else {
            self.b.call(ret_id, &desc)?
        };
        self.charge();
        self.cost += self.mult * callee_cost;
        match ret {
            Some(_) if self.rng().chance(25) => self.hit("call:unused-result"),
            Some(Ty::Int(bits)) => self.finish_int(value, bits)?,
            Some(ty) => self.push_fl(value, ty, Fl { mag: BOUND, nonneg: false, nz: false }),
            None => {}
        }
        Ok(())
    }

    /// One operation that stays in the current block.
    fn op(&mut self) -> IrResult<()> {
        let opt = &self.g.opt;
        let menu: [(u64, bool); 12] = [
            (30, true),
            (8, opt.compares),
            (6, opt.selects),
            (6, opt.casts),
            (10, opt.floats),
            (16, opt.memory),
            (3, opt.ptr_int),
            (6, opt.intrinsics),
            (6, opt.calls),
            (5, opt.hosts),
            (1, true),
            // Last, so that with it off the rolls are what they were.
            (6, opt.vectors),
        ];
        let total: u64 = menu.iter().filter(|(_, on)| *on).map(|(weight, _)| weight).sum();
        let mut roll = self.rng().below(total);
        let mut choice = 0;
        for (at, &(weight, on)) in menu.iter().enumerate() {
            if !on {
                continue;
            }
            if roll < weight {
                choice = at;
                break;
            }
            roll -= weight;
        }
        match choice {
            0 => {
                let bits = self.int_width();
                let value = self.int_binary(None, bits)?;
                self.finish_int(value, bits)
            }
            1 => {
                let value = self.icmp()?;
                self.hit("icmp:value");
                self.finish_int(value, 1)
            }
            2 => self.select(),
            3 => {
                let ints = self.g.ints.clone();
                let (from, to) = (self.rng().pick(&ints), self.rng().pick(&ints));
                let arg = self.pick_int(from)?;
                let value = self.resize(arg, from, to)?;
                if from == to { Ok(()) } else { self.finish_int(value, to) }
            }
            4 => self.float_op(),
            5 => self.mem_op(),
            6 => self.ptr_int(),
            7 => self.intrinsic(),
            8 => self.call(),
            9 => self.host_call(),
            10 => {
                let bits = self.int_width();
                let arg = self.pick_int(bits)?;
                let ty = self.tid(Ty::Int(bits))?;
                let value = self.b.freeze(ty, arg)?;
                self.hit("freeze");
                self.finish_int(value, bits)
            }
            _ => self.vector_op(),
        }
    }

    // ---- vectors (§5.10) ----

    /// The vector type of `len` lanes of `bits` bits.
    fn vector_ty(&mut self, bits: u32, len: u32) -> IrResult<TypeId> {
        let elem = self.tid(Ty::Int(bits))?;
        self.g.m.types.vector(elem, len)
    }

    /// The vector of `len` lanes, each `value`.
    fn vector_splat(&mut self, bits: u32, len: u32, value: u128) -> IrResult<Value> {
        let ty = self.vector_ty(bits, len)?;
        let lane = self.cint(bits, value)?;
        let lane = lane.as_const().expect("an integer constant");
        let m = &mut self.g.m;
        Value::constant(m.consts.splat(&m.types, ty, lane)?)
    }

    /// A vector of `len` lanes of `bits` bits from pool integers: one
    /// broadcast by a `shufflevector`, a few more inserted.
    fn vector_of(&mut self, bits: u32, len: u32) -> IrResult<Value> {
        let ty = self.vector_ty(bits, len)?;
        let m = &mut self.g.m;
        let zero = Value::constant(m.consts.zero(&m.types, ty)?)?;
        let first = self.pick_int(bits)?;
        let at = self.cint(32, 0)?;
        let one = self.b.insert_element(ty, zero, first, at)?;
        let mut vector = self.b.shuffle_vector(ty, one, zero, &vec![0; len as usize])?;
        for _ in 0..self.rng().below(u64::from(len.min(3)) + 1) {
            let lane = self.pick_int(bits)?;
            let index_bits = self.rng().pick(&[32, 64]);
            let at = self.rng().below(u64::from(len));
            let index = self.cint(index_bits, u128::from(at))?;
            vector = self.b.insert_element(ty, vector, lane, index)?;
        }
        self.hit("vector:build");
        Ok(vector)
    }

    /// Vector operations whose operands cannot make them trap (§9.2),
    /// back to a scalar in the pool.
    fn vector_op(&mut self) -> IrResult<()> {
        use Opcode::{AShr, Add, And, LShr, Mul, Or, SExt, Shl, Sub, Trunc, UDiv, URem, Xor, ZExt};
        let widths: Vec<u32> =
            [8, 16, 32, 64].into_iter().filter(|bits| self.g.ints.contains(bits)).collect();
        let mut bits = self.rng().pick(&widths);
        let mut len = self.rng().pick(&[1, 2, 3, 4, 4, 8, 8, 16]);
        let ty = self.vector_ty(bits, len)?;
        let (a, b) = (self.vector_of(bits, len)?, self.vector_of(bits, len)?);
        let opt = self.g.opt.clone();
        let value = match self.rng().below(6) {
            1 if opt.compares => {
                let mask = self.g.m.types.vector(TypeId::I1, len)?;
                let pred = self.rng().pick(IntPredicate::ALL);
                let cond = self.b.icmp(pred, mask, a, b)?;
                self.hit("vector:compare");
                if opt.selects && self.rng().chance(60) {
                    self.hit("vector:select");
                    self.b.select(ty, cond, a, b)?
                } else if self.g.ints.contains(&len) && self.rng().chance(50) {
                    // `<N x i1>` is N bits, lane 0 the lowest (§3.1).
                    let int = self.tid(Ty::Int(len))?;
                    let value = self.b.cast(Opcode::BitCast, int, cond)?;
                    self.hit("vector:mask-bitcast");
                    return self.finish_int(value, len);
                } else {
                    self.b.cast(SExt, ty, cond)?
                }
            }
            2 => {
                let to = self.rng().pick(&[1, 2, 4, len, 2 * len]);
                let mask: Vec<u32> =
                    (0..to).map(|_| self.rng().below(2 * u64::from(len)) as u32).collect();
                let result = self.vector_ty(bits, to)?;
                len = to;
                self.hit("vector:shuffle");
                self.b.shuffle_vector(result, a, b, &mask)?
            }
            3 if opt.casts && widths.len() > 1 => {
                let to = self.rng().pick(&widths);
                let op = if to < bits {
                    Trunc
                } else if self.rng().chance(50) {
                    ZExt
                } else {
                    SExt
                };
                let result = self.vector_ty(to, len)?;
                let same = to == bits;
                bits = to;
                if same {
                    a
                } else {
                    self.hit("vector:cast");
                    self.b.cast(op, result, a)?
                }
            }
            4 if opt.intrinsics => {
                let no = self.cint(1, 0)?;
                let suffix = format!("v{len}i{bits}");
                self.hit("vector:intrinsic");
                match self.rng().below(3) {
                    0 => {
                        let name = self.rng().pick(&["ctpop", "bitreverse"]);
                        self.intrinsic_call(&format!("llvm.{name}.{suffix}"), ty, &[ty], &[a])?
                    }
                    1 => {
                        // With the flag off, zero and the smallest value
                        // are defined.
                        let name = self.rng().pick(&["ctlz", "cttz", "abs"]);
                        let params = [ty, TypeId::I1];
                        self.intrinsic_call(
                            &format!("llvm.{name}.{suffix}"),
                            ty,
                            &params,
                            &[a, no],
                        )?
                    }
                    _ => {
                        let name = self.rng().pick(&[
                            "smax", "smin", "umax", "umin", "sadd.sat", "uadd.sat", "ssub.sat",
                            "usub.sat",
                        ]);
                        let name = format!("llvm.{name}.{suffix}");
                        self.intrinsic_call(&name, ty, &[ty, ty], &[a, b])?
                    }
                }
            }
            _ => {
                let op =
                    self.rng().pick(&[Add, Sub, Mul, And, Or, Xor, Shl, LShr, AShr, UDiv, URem]);
                let rhs = match op {
                    Shl | LShr | AShr => {
                        let limit = self.vector_splat(bits, len, u128::from(bits - 1))?;
                        self.b.binary(And, ty, b, limit)?
                    }
                    UDiv | URem => {
                        let one = self.vector_splat(bits, len, 1)?;
                        self.b.binary(Or, ty, b, one)?
                    }
                    _ => b,
                };
                self.hit("vector:int");
                self.b.binary(op, ty, a, rhs)?
            }
        };
        self.vector_to_int(value, bits, len)
    }

    /// A scalar of the pool from `vector`: a lane, a reduction of the
    /// lanes, or all of it.
    fn vector_to_int(&mut self, vector: Value, bits: u32, len: u32) -> IrResult<()> {
        let ty = self.vector_ty(bits, len)?;
        let elem = self.tid(Ty::Int(bits))?;
        let whole = bits * len;
        match self.rng().below(3) {
            0 if self.g.ints.contains(&whole) => {
                let int = self.tid(Ty::Int(whole))?;
                let value = self.b.cast(Opcode::BitCast, int, vector)?;
                self.hit("vector:bitcast");
                self.finish_int(value, whole)
            }
            1 => {
                let op = self
                    .rng()
                    .pick(&["add", "mul", "and", "or", "xor", "smax", "smin", "umax", "umin"]);
                let name = format!("llvm.vector.reduce.{op}.v{len}i{bits}");
                let value = self.intrinsic_call(&name, elem, &[ty], &[vector])?;
                self.hit("vector:reduce");
                self.finish_int(value, bits)
            }
            _ => {
                let at = self.rng().below(u64::from(len));
                let index = self.cint(64, u128::from(at))?;
                let value = self.b.extract_element(elem, vector, index)?;
                self.hit("vector:extract");
                self.finish_int(value, bits)
            }
        }
    }

    // ---- exceptions (§5.9) ----

    /// The steps an exception may take to unwind (§5.9): two for each
    /// frame of the deepest call chain, and some more.
    fn charge_throw(&mut self) {
        self.charge();
        self.cost += self.mult * (2 * self.g.fns.len() as u64 + 8);
    }

    /// The call of `desc`, of a callee that may end in the exceptions
    /// `escapes`: a call, `nounwind` when nothing can escape, or an
    /// `invoke` whose landing pad rejoins the normal destination with a
    /// `phi` of the result. `None` when no call fits here: the entry
    /// function lets nothing escape, and pads nest only so deep.
    fn eh_call(
        &mut self,
        desc: &CallDesc<'_>,
        ret: Option<Ty>,
        ret_id: TypeId,
        escapes: u8,
    ) -> IrResult<Option<Value>> {
        let contained = self.index == 0;
        if escapes == 0 {
            let nounwind = self.rng().chance(30);
            if nounwind {
                self.hit("eh:nounwind-call");
            }
            return Ok(Some(self.b.call(ret_id, &CallDesc { nounwind, ..*desc })?));
        }
        let invoke = self.pads < 2 && (contained || self.rng().chance(60));
        if !invoke {
            if contained {
                return Ok(None);
            }
            self.escapes |= escapes;
            return Ok(Some(self.b.call(ret_id, desc)?));
        }
        let from = self.here();
        let (normal, pad) = (self.b.create_block()?, self.b.create_block()?);
        let value = self.b.invoke(ret_id, desc, normal, pad)?;
        self.hit("eh:invoke-call");
        let types: Vec<Ty> = ret.into_iter().collect();
        let caught =
            self.landing(pad, escapes, &[], &[(from, Vec::new())], normal, &types, MAX_DEPTH)?;
        self.b.begin_block(normal)?;
        let Some(ty) = ret else { return Ok(Some(value)) };
        let id = self.tid(ty)?;
        let phi = self.b.phi(id, &[(value, from), (caught.1[0].value, caught.0)])?;
        self.hit("phi");
        self.hit("eh:normal-phi");
        Ok(Some(phi))
    }

    /// A landing pad in block `pad` for the exceptions `incoming`, with
    /// `phi`s of `pad_types` from `froms`: random clauses, a dispatch on
    /// the selector through `llvm.eh.typeid.for` and its sign, a cleanup
    /// that resumes, and a handler that catches the exception, runs a
    /// region and branches to `join` with values of `types`. Returns the
    /// handler's last block and those values.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn landing(
        &mut self,
        pad: BlockId,
        incoming: u8,
        pad_types: &[Ty],
        froms: &[(BlockId, Vec<Avail>)],
        join: BlockId,
        types: &[Ty],
        depth: u32,
    ) -> IrResult<(BlockId, Vec<Avail>)> {
        let eh = self.g.eh.expect("exceptions are on");
        let scope = self.scope();
        self.pads += 1;
        self.b.begin_block(pad)?;
        if !pad_types.is_empty() {
            self.phis(pad_types, froms)?;
            self.hit("eh:pad-phi");
        }
        let live: Vec<Avail> = self.pool[self.pool.len() - pad_types.len()..].to_vec();
        // The clauses, and which of the incoming types one handles.
        let mut clauses = Vec::new();
        let mut handled = 0u8;
        for _ in 0..=self.rng().below(3) {
            match self.rng().below(100) {
                0..50 => {
                    let t = self.rng().below(4) as usize;
                    clauses.push(Clause::Catch(self.g.m.consts.addr(eh.type_infos[t], 0)?));
                    handled |= 1 << t;
                    self.hit("eh:catch");
                }
                50..65 => {
                    clauses
                        .push(Clause::Catch(self.g.m.consts.null(&self.g.m.types, TypeId::PTR)?));
                    handled = 0xf;
                    self.hit("eh:catch-all");
                }
                roll => {
                    // A handler for the types it does not list.
                    let listed = if roll < 85 { 1 + self.rng().below(15) as u8 } else { 0 };
                    let mut elems = Vec::new();
                    for t in 0..4 {
                        if listed & (1 << t) != 0 {
                            elems.push(self.g.m.consts.addr(eh.type_infos[t], 0)?);
                        }
                    }
                    let m = &mut self.g.m;
                    let array = m.types.array(TypeId::PTR, elems.len() as u64)?;
                    let filter = if elems.is_empty() {
                        m.consts.zero(&m.types, array)?
                    } else {
                        m.consts.aggregate(&m.types, array, &elems)?
                    };
                    clauses.push(Clause::Filter(filter));
                    handled |= !listed & 0xf;
                    self.hit(if listed == 0 { "eh:filter-empty" } else { "eh:filter" });
                }
            }
        }
        let cleanup = self.rng().chance(50);
        let mut unhandled = incoming & !handled;
        if self.index == 0 && unhandled != 0 {
            // Nothing leaves the entry function.
            clauses.push(Clause::Catch(self.g.m.consts.null(&self.g.m.types, TypeId::PTR)?));
            self.hit("eh:catch-all");
            unhandled = 0;
        }
        self.escapes |= unhandled;
        let pair = self.b.landing_pad(eh.pad_ty, cleanup, &clauses)?;
        // Which pad was entered and the values live into it, observed.
        let mark = 0x9AD0_0000 + ((self.index as u128) << 16) + pad.index() as u128;
        let mark = self.cint(64, mark)?;
        self.sink(mark)?;
        for avail in live {
            let value = match avail.ty {
                Ty::Int(64) => avail.value,
                Ty::Int(bits @ 1..64) if self.g.opt.casts => self.resize(avail.value, bits, 64)?,
                _ => continue,
            };
            self.sink(value)?;
        }
        let exn = self.b.extract_value(TypeId::PTR, pair, 0)?;
        let sel = self.b.extract_value(TypeId::I32, pair, 8)?;
        // The dispatch: one type, then a catch, a filter, a cleanup.
        let t = self.rng().below(4) as usize;
        let info = [Value::sym(eh.type_infos[t])?];
        let id =
            self.intrinsic_call("llvm.eh.typeid.for.p0", TypeId::I32, &[TypeId::PTR], &info)?;
        self.hit("eh:typeid-for");
        let (caught, positive, negative, clean) = (
            self.b.create_block()?,
            self.b.create_block()?,
            self.b.create_block()?,
            self.b.create_block()?,
        );
        let is_t = self.b.icmp(IntPredicate::Eq, TypeId::I1, sel, id)?;
        let first = self.here();
        self.b.cond_br(is_t, caught, positive)?;
        self.b.begin_block(positive)?;
        let zero = self.cint(32, 0)?;
        let is_catch = self.b.icmp(IntPredicate::Sgt, TypeId::I1, sel, zero)?;
        self.b.cond_br(is_catch, caught, negative)?;
        self.b.begin_block(negative)?;
        let is_filter = self.b.icmp(IntPredicate::Slt, TypeId::I1, sel, zero)?;
        self.b.cond_br(is_filter, caught, clean)?;
        self.b.begin_block(clean)?;
        if cleanup {
            let inner = self.scope();
            let budget = 2 + self.rng().below(6) as usize;
            self.region(budget, MAX_DEPTH)?;
            self.leave(inner);
            self.hit("eh:cleanup");
        }
        self.b.resume(pair)?;
        self.b.begin_block(caught)?;
        let tags = [self.cint(64, 1)?, self.cint(64, 2)?, self.cint(64, 3)?];
        let tag = self
            .b
            .phi(TypeId::I64, &[(tags[0], first), (tags[1], positive), (tags[2], negative)])?;
        self.hit("phi");
        self.sink(tag)?;
        self.push(tag, Ty::Int(64));
        let args = [exn];
        let payload =
            self.b.call(TypeId::I64, &CallDesc::new(Value::sym(eh.catch)?, eh.catch_ty, &args))?;
        self.push(payload, Ty::Int(64));
        let budget = 2 + self.rng().below(10) as usize;
        self.region(budget, depth.max(1))?;
        let values = self.merge_values(types)?;
        let end = self.here();
        self.b.br(join)?;
        self.pads -= 1;
        self.leave(scope);
        Ok((end, values))
    }

    /// One or two `invoke`s of `hod_throw`, each under a condition, to one
    /// landing pad whose handler joins the code after them.
    fn try_throw(&mut self, depth: u32) -> IrResult<()> {
        let eh = self.g.eh.expect("exceptions are on");
        let types = self.merge_types();
        let pad_types = if self.rng().chance(40) { self.merge_types() } else { Vec::new() };
        let throws = if self.rng().chance(30) { 2 } else { 1 };
        let (pad, never, join) =
            (self.b.create_block()?, self.b.create_block()?, self.b.create_block()?);
        let mut incoming = 0u8;
        let mut froms = Vec::new();
        // What the blocks of the chain define dominates neither the pad
        // nor the join.
        let chain = self.scope();
        for _ in 0..throws {
            let cond = self.cond()?;
            let (site, next) = (self.b.create_block()?, self.b.create_block()?);
            self.b.cond_br(cond, site, next)?;
            self.b.begin_block(site)?;
            let scope = self.scope();
            let t = self.rng().below(4) as usize;
            incoming |= 1 << t;
            let payload = self.pick_int(64)?;
            let values = self.merge_values(&pad_types)?;
            let args = [Value::sym(eh.type_infos[t])?, payload];
            let desc = CallDesc::new(Value::sym(eh.throw)?, eh.throw_ty, &args);
            self.b.invoke(TypeId::VOID, &desc, never, pad)?;
            froms.push((site, values));
            self.leave(scope);
            self.charge_throw();
            self.hit("eh:invoke-throw");
            self.b.begin_block(next)?;
        }
        if throws == 2 {
            self.hit("eh:shared-pad");
        }
        let skip = (self.here(), self.merge_values(&types)?);
        self.b.br(join)?;
        self.leave(chain);
        self.b.begin_block(never)?;
        self.b.unreachable()?;
        let caught = self.landing(pad, incoming, &pad_types, &froms, join, &types, depth + 1)?;
        self.b.begin_block(join)?;
        self.phis(&types, &[skip, caught])
    }

    /// A call of `hod_throw` without a landing pad, under a condition:
    /// the exception leaves the function.
    fn throw_plain(&mut self) -> IrResult<()> {
        let eh = self.g.eh.expect("exceptions are on");
        let cond = self.cond()?;
        let (site, on) = (self.b.create_block()?, self.b.create_block()?);
        self.b.cond_br(cond, site, on)?;
        self.b.begin_block(site)?;
        let scope = self.scope();
        let t = self.rng().below(4) as usize;
        let payload = self.pick_int(64)?;
        let args = [Value::sym(eh.type_infos[t])?, payload];
        self.b.call(TypeId::VOID, &CallDesc::new(Value::sym(eh.throw)?, eh.throw_ty, &args))?;
        self.b.unreachable()?;
        self.leave(scope);
        self.escapes |= 1 << t;
        self.charge_throw();
        self.hit("eh:throw");
        self.b.begin_block(on)
    }

    // ---- block addresses (§5.5) ----

    /// A computed goto: an `indirectbr` to an address loaded from a
    /// constant table of `blockaddress`es, with arms that join.
    fn computed_goto(&mut self, budget: usize, depth: u32) -> IrResult<()> {
        let n: u64 = if self.rng().chance(50) { 2 } else { 4 };
        let types = self.merge_types();
        let mut targets = Vec::new();
        for _ in 0..n {
            targets.push(self.b.create_block()?);
        }
        let join = self.b.create_block()?;
        let sym = self.g.fns[self.index].sym;
        let mut addrs = Vec::new();
        for &target in &targets {
            addrs.push(self.g.m.consts.block_addr(sym, target)?);
        }
        let m = &mut self.g.m;
        let array = m.types.array(TypeId::PTR, n)?;
        let init = m.consts.aggregate(&m.types, array, &addrs)?;
        let name = format!("bt{}_{}", self.index, self.gotos);
        self.gotos += 1;
        let desc = VariableDesc {
            init: Some(init),
            constant: true,
            align: Some(8),
            ..VariableDesc::new(array)
        };
        let table = m.add_variable(name.as_bytes(), &desc)?;
        self.hit("blockaddress:table");
        let any = self.pick_int(64)?;
        let low = self.cint(64, u128::from(n - 1))?;
        let index = self.bin(Opcode::And, 64, any, low)?;
        let at = self.b.ptr_add(TypeId::PTR, Value::sym(table)?, 0, &[(index, 8)])?;
        let addr = self.b.load(TypeId::PTR, at, aligned(8)?)?;
        // Comparing with `null` is all that is defined (§5.5).
        let null = Value::constant(self.g.m.consts.null(&self.g.m.types, TypeId::PTR)?)?;
        let pred = if self.rng().chance(50) { IntPredicate::Eq } else { IntPredicate::Ne };
        let is = self.b.icmp(pred, TypeId::I1, addr, null)?;
        self.hit("blockaddress:compare");
        self.finish_int(is, 1)?;
        self.b.indirect_br(addr, &targets)?;
        self.hit("cf:indirectbr");
        let mut edges = Vec::new();
        for &target in &targets {
            edges.push(self.arm(target, join, &types, budget / n as usize, depth)?);
        }
        self.b.begin_block(join)?;
        self.phis(&types, &edges)
    }

    // ---- memory of the function ----

    /// The function's stack slots, each written in full in the entry
    /// block: one byte region and some arrays of cells.
    fn make_slots(&mut self) -> IrResult<()> {
        for _ in 0..=self.rng().below(2) {
            let slot = self.b.add_slot(16, 8)?;
            let base = self.b.slot(slot)?;
            self.hit("mem:slot");
            for half in 0..2 {
                let value = self.pick_int(64)?;
                let at = self.b.ptr_add(TypeId::PTR, base, half * 8, &[])?;
                self.b.store(value, at, aligned(8)?)?;
            }
            self.arrs.push(Arr {
                base,
                offset: 0,
                elem: Elem::Bytes,
                len: 16,
                align: 8,
                writable: true,
                global: false,
            });
        }
        for _ in 0..self.rng().below(4) {
            let ty = self.g.value_ty();
            let id = self.tid(ty)?;
            let (stride, align) = (self.g.m.types.alloc_size_of(id), self.g.m.types.align_of(id));
            let len = self.rng().pick(&[1u64, 2, 4]);
            let slot = self.b.add_slot(stride * len, align)?;
            let base = self.b.slot(slot)?;
            self.hit("mem:slot");
            for at in 0..len {
                let value = Value::constant(self.g.cell_const(ty)?)?;
                let place = self.b.ptr_add(TypeId::PTR, base, (at * stride) as i64, &[])?;
                self.b.store(value, place, aligned(align)?)?;
            }
            self.arrs.push(Arr {
                base,
                offset: 0,
                elem: Elem::Cell(ty),
                len,
                align,
                writable: true,
                global: false,
            });
        }
        Ok(())
    }

    // ---- control flow ----

    /// Code of about `budget` instructions that starts in the current
    /// block and ends in a block that is open.
    fn region(&mut self, budget: usize, depth: u32) -> IrResult<()> {
        let end = self.b.inst_count() + budget;
        while self.b.inst_count() < end {
            let left = end - self.b.inst_count();
            let opt = &self.g.opt;
            let nest = depth < MAX_DEPTH && left >= 10;
            let (branches, loops, switches) =
                (nest && opt.branches, nest && opt.loops, nest && opt.switches);
            let dead_end = opt.unreachable_blocks;
            let (tries, gotos) = (nest && opt.exceptions, nest && opt.block_addresses);
            let throws = tries && self.index != 0;
            match self.rng().below(100) {
                0..3 if branches => self.diamond(left / 3, depth)?,
                3..5 if branches => self.triangle(left / 3, depth)?,
                5..6 if branches => self.condbr_twice()?,
                6..7 if branches => self.dead_trap()?,
                7..10 if loops => self.counted_loop(left / 3, depth, false)?,
                10..12 if loops => self.counted_loop(4, depth, true)?,
                12..15 if switches => self.switch(left / 4, depth)?,
                15..16 if dead_end => self.unreachable_block()?,
                16..19 if tries => self.try_throw(depth)?,
                19..20 if throws => self.throw_plain()?,
                20..22 if gotos => self.computed_goto(left / 3, depth)?,
                _ => self.op()?,
            }
        }
        Ok(())
    }

    /// The pool as it must be again when the blocks built from here on
    /// stop dominating the code that follows.
    fn scope(&self) -> usize {
        self.pool.len()
    }

    fn leave(&mut self, scope: usize) {
        self.pool.truncate(scope);
    }

    fn merge_types(&mut self) -> Vec<Ty> {
        (0..=self.rng().below(3)).map(|_| self.merge_ty()).collect()
    }

    /// One value of each type, for the `phi`s of a block the current one
    /// branches to.
    fn merge_values(&mut self, types: &[Ty]) -> IrResult<Vec<Avail>> {
        types.iter().map(|&ty| self.pick(ty, BOUND)).collect()
    }

    /// The `phi`s of the current block: `phi` number `n` gets `edges[e].1[n]`
    /// from block `edges[e].0`.
    fn phis(&mut self, types: &[Ty], edges: &[(BlockId, Vec<Avail>)]) -> IrResult<()> {
        for (at, &ty) in types.iter().enumerate() {
            let id = self.tid(ty)?;
            let incoming: Vec<_> =
                edges.iter().map(|(block, values)| (values[at].value, *block)).collect();
            let value = self.b.phi(id, &incoming)?;
            self.hit("phi");
            let fl = edges.iter().map(|(_, values)| values[at].fl).reduce(merge_fl);
            self.push_fl(value, ty, fl.unwrap_or(NO_FL));
        }
        Ok(())
    }

    fn here(&self) -> BlockId {
        self.b.current_block().expect("a block is open between constructs")
    }

    /// An arm: a region in `block` that ends with a branch to `to`.
    /// Returns the block the branch is in and the values for the `phi`s.
    fn arm(
        &mut self,
        block: BlockId,
        to: BlockId,
        types: &[Ty],
        budget: usize,
        depth: u32,
    ) -> IrResult<(BlockId, Vec<Avail>)> {
        self.b.begin_block(block)?;
        let scope = self.scope();
        self.region(budget, depth + 1)?;
        let values = self.merge_values(types)?;
        let end = self.here();
        self.b.br(to)?;
        self.leave(scope);
        Ok((end, values))
    }

    fn diamond(&mut self, budget: usize, depth: u32) -> IrResult<()> {
        let types = self.merge_types();
        let cond = self.cond()?;
        let (yes, no, join) =
            (self.b.create_block()?, self.b.create_block()?, self.b.create_block()?);
        self.b.cond_br(cond, yes, no)?;
        let edges = [
            self.arm(yes, join, &types, budget, depth)?,
            self.arm(no, join, &types, budget, depth)?,
        ];
        self.b.begin_block(join)?;
        self.hit("cf:diamond");
        self.phis(&types, &edges)
    }

    /// A branch around one arm: the edge that skips it leaves a block
    /// with two successors for a block with two predecessors.
    fn triangle(&mut self, budget: usize, depth: u32) -> IrResult<()> {
        let types = self.merge_types();
        let cond = self.cond()?;
        let before = (self.here(), self.merge_values(&types)?);
        let (side, join) = (self.b.create_block()?, self.b.create_block()?);
        if self.rng().chance(50) {
            self.b.cond_br(cond, side, join)?;
        } else {
            self.b.cond_br(cond, join, side)?;
        }
        let edges = [before, self.arm(side, join, &types, budget, depth)?];
        self.b.begin_block(join)?;
        self.hit("cf:critical-edge");
        self.phis(&types, &edges)
    }

    /// A conditional branch whose two edges reach one block.
    fn condbr_twice(&mut self) -> IrResult<()> {
        let types = self.merge_types();
        let cond = self.cond()?;
        let before = (self.here(), self.merge_values(&types)?);
        let join = self.b.create_block()?;
        self.b.cond_br(cond, join, join)?;
        self.b.begin_block(join)?;
        self.hit("cf:condbr-twice");
        self.phis(&types, &[before.clone(), before])
    }

    /// A branch on a condition that always holds, whose other edge
    /// reaches an `unreachable`.
    fn dead_trap(&mut self) -> IrResult<()> {
        let any = self.pick_int(64)?;
        let three = self.cint(64, 3)?;
        let low = self.bin(Opcode::And, 64, any, three)?;
        let four = self.cint(64, 4)?;
        let cond = self.b.icmp(IntPredicate::Ult, TypeId::I1, low, four)?;
        let (on, never) = (self.b.create_block()?, self.b.create_block()?);
        self.b.cond_br(cond, on, never)?;
        self.b.begin_block(never)?;
        self.b.unreachable()?;
        self.b.begin_block(on)?;
        self.hit("cf:dead-trap");
        self.hit("icmp:branch");
        Ok(())
    }

    /// A block without predecessors that joins the code.
    fn unreachable_block(&mut self) -> IrResult<()> {
        let types: Vec<Ty> = self.merge_types().into_iter().filter(|&ty| ty != Ty::Bytes).collect();
        let before = (self.here(), self.merge_values(&types)?);
        let (island, join) = (self.b.create_block()?, self.b.create_block()?);
        self.b.br(join)?;
        self.b.begin_block(island)?;
        // Constants only: nothing reaches this block to define anything.
        let mut values = Vec::new();
        for &ty in &types {
            let value = Value::constant(self.g.cell_const(ty)?)?;
            values.push(Avail {
                value,
                ty,
                fl: Fl { mag: 4, nonneg: false, nz: false },
                once: false,
                dead: false,
            });
        }
        let (a, b) = (self.rng().interesting(64), self.rng().interesting(64));
        let (a, b) = (self.cint(64, a)?, self.cint(64, b)?);
        self.bin(Opcode::Add, 64, a, b)?;
        self.b.br(join)?;
        self.b.begin_block(join)?;
        self.hit("cf:unreachable-block");
        self.phis(&types, &[before, (island, values)])
    }

    /// A loop that runs one to four times, around a region or, when
    /// `single`, as one block that is its own successor. Its `phi`s
    /// include a pair that trade values on every back edge.
    fn counted_loop(&mut self, budget: usize, depth: u32, single: bool) -> IrResult<()> {
        self.charge();
        // Room for the body `MAX_TRIP` times over, and for the calls a
        // body of this size may hold.
        let body_cost = self.mult * MAX_TRIP * (budget as u64 + 40);
        if self.cost + body_cost > self.g.opt.max_fuel / 2 {
            return self.op();
        }
        let counter_bits = self.int_width();
        let counter_bits = if (7..=64).contains(&counter_bits) { counter_bits } else { 64 };
        let counter = Ty::Int(counter_bits);
        // The trip count: a constant, or one to four from a value.
        let trips = if self.rng().chance(50) {
            let trips = 1 + self.rng().below(MAX_TRIP);
            self.cint(counter_bits, u128::from(trips))?
        } else {
            let any = self.pick_int(counter_bits)?;
            let three = self.cint(counter_bits, u128::from(MAX_TRIP - 1))?;
            let low = self.bin(Opcode::And, counter_bits, any, three)?;
            let one = self.cint(counter_bits, 1)?;
            self.bin(Opcode::Add, counter_bits, low, one)?
        };
        // Carried values: a swapped pair of one type, then some others.
        let pair = self.g.value_ty();
        let mut types = vec![pair, pair];
        types.extend(self.merge_types());
        let inits = self.merge_values(&types)?;
        let zero = self.cint(counter_bits, 0)?;
        let before = self.here();
        let (head, exit) = (self.b.create_block()?, self.b.create_block()?);
        self.b.br(head)?;
        self.b.begin_block(head)?;
        self.charge();
        self.mult *= MAX_TRIP;
        let counter_id = self.tid(counter)?;
        let index = self.b.phi(counter_id, &[(zero, before)])?;
        let mut carried = Vec::new();
        for (&ty, init) in types.iter().zip(&inits) {
            let id = self.tid(ty)?;
            let value = self.b.phi(id, &[(init.value, before)])?;
            self.hit("phi");
            // What comes around the loop is held to the same bound.
            let fl = Fl { mag: init.fl.mag.max(BOUND), nonneg: false, nz: false };
            carried.push(Avail { value, ty, fl, once: false, dead: false });
        }
        let scope = self.scope();
        self.pool.extend(carried.iter().copied());
        self.push(index, counter);
        if single {
            for _ in 0..budget {
                self.op()?;
            }
        } else {
            self.region(budget, depth + 1)?;
        }
        // What each carried value is next time: the pair trade places.
        let mut nexts = vec![carried[1].value, carried[0].value];
        for (at, &ty) in types.iter().enumerate().skip(2) {
            nexts.push(self.pick(ty, carried[at].fl.mag)?.value);
        }
        let one = self.cint(counter_bits, 1)?;
        let next_index = self.bin(Opcode::Add, counter_bits, index, one)?;
        let again = self.b.icmp(IntPredicate::Ult, TypeId::I1, next_index, trips)?;
        self.hit("icmp:branch");
        let latch = self.here();
        self.b.cond_br(again, head, exit)?;
        self.b.add_incoming(index, next_index, latch)?;
        for (phi, next) in carried.iter().zip(nexts) {
            self.b.add_incoming(phi.value, next, latch)?;
        }
        self.charge();
        self.mult /= MAX_TRIP;
        // The header dominates what follows; the body does not.
        self.leave(scope);
        self.pool.extend(carried.iter().copied());
        self.b.begin_block(exit)?;
        self.hit(if single { "cf:self-loop" } else { "cf:loop" });
        self.hit("cf:swap");
        Ok(())
    }

    /// A `switch` with dense or sparse cases, some of which share a
    /// destination that has `phi`s, and some of which skip to the join.
    fn switch(&mut self, budget: usize, depth: u32) -> IrResult<()> {
        let widths: Vec<u32> =
            self.g.ints.iter().copied().filter(|bits| (7..=64).contains(bits)).collect();
        let bits = self.rng().pick(&widths);
        let dense = self.rng().chance(50);
        let any = self.pick_int(bits)?;
        let limit = if dense { 7 } else { 63 };
        let limit_value = self.cint(bits, limit)?;
        let on = self.bin(Opcode::And, bits, any, limit_value)?;
        let mut keys: Vec<u128> = Vec::new();
        if dense {
            keys.extend(0..3 + u128::from(self.rng().below(4)));
        } else {
            while keys.len() < 4 {
                let key = u128::from(self.rng().below(64));
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        let types = self.merge_types();
        let before = (self.here(), self.merge_values(&types)?);
        let join = self.b.create_block()?;
        let blocks = [self.b.create_block()?, self.b.create_block()?, self.b.create_block()?];
        // Where each case goes: a block, or the join directly. The first
        // two cases share the first block, the third has the second.
        let mut edges_to = [0usize; 3];
        let mut direct = 0;
        let mut cases = Vec::new();
        let int_ty = self.tid(Ty::Int(bits))?;
        for (at, &key) in keys.iter().enumerate() {
            let to = match at {
                0 | 1 => 0,
                2 => 1,
                _ => self.rng().below(4) as usize,
            };
            let target = if to == 3 {
                direct += 1;
                join
            } else {
                edges_to[to] += 1;
                blocks[to]
            };
            let m = &mut self.g.m;
            cases.push((m.consts.int(&m.types, int_ty, key as u64)?, target));
        }
        // The default: the last block, so that every block is reached.
        edges_to[2] += 1;
        self.b.switch(on, blocks[2], &cases)?;
        let mut edges = Vec::new();
        for _ in 0..direct {
            edges.push(before.clone());
        }
        for (at, &block) in blocks.iter().enumerate() {
            self.b.begin_block(block)?;
            let scope = self.scope();
            // One `phi` entry for each case that comes here.
            let shared: Vec<_> = (0..edges_to[at]).map(|_| before.clone()).collect();
            self.phis(&types, &shared)?;
            if edges_to[at] > 1 {
                self.hit("cf:switch-shared");
            }
            self.region(budget, depth + 1)?;
            let values = self.merge_values(&types)?;
            let end = self.here();
            self.b.br(join)?;
            self.leave(scope);
            edges.push((end, values));
        }
        self.b.begin_block(join)?;
        self.hit(if dense { "cf:switch-dense" } else { "cf:switch-sparse" });
        self.phis(&types, &edges)
    }

    // ---- the end of the function ----

    /// Folds values into one `i64`, reports it, and returns it as the
    /// function's return type.
    fn finish(&mut self, sig: usize) -> IrResult<()> {
        // Multiplying by an odd number loses nothing of what was folded
        // in before.
        let mut acc = self.observe()?;
        // More of them in a larger function.
        let folds = 3 + self.rng().below(6) + u64::from(self.g.opt.insts / 50).min(16);
        for _ in 0..folds {
            let odd = self.rng().next() | 1;
            let odd = self.cint(64, u128::from(odd))?;
            let spread = self.bin(Opcode::Mul, 64, acc, odd)?;
            let more = self.observe()?;
            let op = self.rng().pick(&[Opcode::Xor, Opcode::Add, Opcode::Sub]);
            acc = self.bin(op, 64, spread, more)?;
        }
        if let Some(std) = self.g.std
            && self.rng().chance(60)
        {
            let args = [acc];
            self.b.call(TypeId::VOID, &CallDesc::new(Value::sym(std.sink)?, std.sink_ty, &args))?;
            self.hit("call:sink");
        }
        let ret = self.g.sigs[sig].ret;
        let value = match ret {
            None => {
                // A function without a result leaves it in memory, if it
                // was given any.
                let given = self
                    .arrs
                    .iter()
                    .find(|arr| matches!(arr.base.kind(), hod_ir::ValueKind::Arg(_)))
                    .map(|arr| arr.base);
                if let Some(base) = given {
                    self.b.store(acc, base, aligned(8)?)?;
                    self.hit("mem:store");
                }
                None
            }
            Some(Ty::Int(64)) => Some(acc),
            Some(Ty::Int(bits)) => Some(self.resize(acc, 64, bits)?),
            Some(ty) => {
                let low = self.cint(64, 0xfff)?;
                let small = self.bin(Opcode::And, 64, acc, low)?;
                Some(self.cast(Opcode::UIToFP, ty, small)?)
            }
        };
        self.b.ret(value)?;
        Ok(())
    }
}

/// Eighth number `n` from -8: for `n` up to 128, a float in [-8, 8].
fn eighth(n: u64) -> f64 {
    f64::from(n as u32) / 8.0 - 8.0
}

/// Where `member` is in a signature's table.
fn mates_place(members: &[usize], member: usize) -> u128 {
    members.iter().position(|&m| m == member).unwrap_or(0) as u128
}

/// The bounds of a value that is one of two.
fn merge_fl(a: Fl, b: Fl) -> Fl {
    Fl { mag: a.mag.max(b.mag), nonneg: a.nonneg && b.nonneg, nz: a.nz && b.nz }
}
