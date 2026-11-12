//! The interpreter proper (`spec/interp.md` §2, §5 to §8): programs, the
//! run loop, calls and host functions.
//!
//! A run is a loop over an explicit stack of frames: an interpreted call
//! pushes a frame and a `ret` pops one, so the host stack does not grow
//! with the interpreted call depth.

use std::collections::HashMap;
use std::fmt::{self, Write as _};
use std::rc::Rc;
use std::sync::Arc;

use hod_ir::{
    BlockId, Body, CallSite, Callee, Clause, Const, ConstId, FixupTarget, FnAttrs, Inst, InstId,
    InstKind, Intrinsic, IrErrorKind, IrResult, Linkage, Module, NoCode, Opcode, ParamMem, RmwOp,
    SymId, SymbolKind, TlsModel, TypeId, TypeKind, Value, ValueKind, no_code_intrinsic,
};

use crate::eh;
use crate::error::{Stop, Trap, invalid, unsupported};
use crate::memory::{AllocKind, Mark, Memory};
use crate::ops::{self, Checked, IntOp};
use crate::value::{Agg, MAX_INT_BITS, Val, sext};

/// A module and the bodies of some of its functions (§2).
#[derive(Clone, Debug)]
pub struct Program<'m> {
    module: &'m Module,
    /// Indexed by `SymId`.
    bodies: Vec<Option<&'m Body>>,
}

impl<'m> Program<'m> {
    /// A program over `module` with no bodies yet.
    pub fn new(module: &'m Module) -> Program<'m> {
        Program { module, bodies: Vec::new() }
    }

    /// A program over `module` with these bodies.
    ///
    /// # Errors
    /// As [`Program::add_body`].
    pub fn with_bodies(
        module: &'m Module,
        bodies: impl IntoIterator<Item = &'m Body>,
    ) -> Result<Program<'m>, Stop> {
        let mut program = Program::new(module);
        bodies.into_iter().try_for_each(|body| program.add_body(body))?;
        Ok(program)
    }

    /// Adds the body of the function `body.symbol()`.
    ///
    /// # Errors
    /// `Invalid` if the symbol is not a function of the module with the
    /// body's function type, or already has a body here.
    pub fn add_body(&mut self, body: &'m Body) -> Result<(), Stop> {
        let sym = body.symbol();
        if sym.index() >= self.module.symbol_count() {
            return invalid(format!("{sym:?} is not a symbol of the module"));
        }
        let symbol = self.module.symbol(sym);
        if !symbol.is_function() || symbol.ty() != body.fn_type() {
            return invalid(format!("{sym:?} is not a function of the body's type"));
        }
        if self.bodies.len() <= sym.index() {
            self.bodies.resize(sym.index() + 1, None);
        }
        if self.bodies[sym.index()].is_some() {
            return invalid(format!("{sym:?} has two bodies"));
        }
        self.bodies[sym.index()] = Some(body);
        Ok(())
    }

    /// The module.
    pub fn module(&self) -> &'m Module {
        self.module
    }

    /// The body of function `sym`, if the program has it.
    pub fn body(&self, sym: SymId) -> Option<&'m Body> {
        self.bodies.get(sym.index()).copied().flatten()
    }

    /// The bodies, in symbol order.
    pub fn bodies(&self) -> impl Iterator<Item = &'m Body> + '_ {
        self.bodies.iter().copied().flatten()
    }

    /// Runs `Body::verify` on every body.
    ///
    /// # Errors
    /// The first violation.
    pub fn verify(&self) -> IrResult<()> {
        self.bodies().try_for_each(|body| body.verify(self.module))
    }
}

/// What a run does with a floating-point operation whose result is a NaN
/// (§5.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum NanPolicy {
    /// Stop with [`Trap::NanResult`]: LLVM leaves the sign and payload of
    /// a computed NaN open, so its bits are not one result.
    #[default]
    Trap,
    /// Continue with the NaN the host's arithmetic produced.
    Host,
}

/// The parameters of a run (§7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Config {
    /// The most instructions a run may execute.
    pub fuel: u64,
    /// The most frames that may be live at once; the entry function's is
    /// the first.
    pub max_call_depth: u32,
    /// The most bytes that may be live at once in module variables, stack
    /// slots and dynamic allocations.
    pub max_memory: u64,
    /// Addresses are handed out upwards from here.
    pub addr_base: u64,
    /// At least this many unused addresses lie between two allocations.
    pub addr_gap: u64,
    /// What a computed NaN does.
    pub nan: NanPolicy,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            fuel: 10_000_000,
            max_call_depth: 1000,
            max_memory: 64 << 20,
            addr_base: 0x1_0000,
            addr_gap: 64,
            nan: NanPolicy::Trap,
        }
    }
}

/// One completed call of a host function (§6).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Event {
    /// The function's symbol name.
    pub name: Arc<str>,
    /// The arguments.
    pub args: Vec<Val>,
    /// What it returned; `None` for a `void` function.
    pub result: Option<Val>,
}

/// `name(i64 1, ptr 0x10040)` or `name() = i64 7`.
impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", self.name)?;
        for (index, arg) in self.args.iter().enumerate() {
            write!(f, "{}{arg}", if index == 0 { "" } else { ", " })?;
        }
        f.write_str(")")?;
        match &self.result {
            Some(result) => write!(f, " = {result}"),
            None => Ok(()),
        }
    }
}

/// What a run did (§6).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// The entry function's return value (`None` for `void`), or why
    /// there is none.
    pub result: Result<Option<Val>, Stop>,
    /// The host calls that completed, in order.
    pub trace: Vec<Event>,
    /// Instructions executed.
    pub steps: u64,
    /// The most frames that were live at once.
    pub max_depth: u32,
    /// The most bytes that were live at once.
    pub peak_memory: u64,
    /// Where the run stopped, when it stopped inside a function.
    pub stopped_at: Option<(SymId, InstId)>,
}

impl Outcome {
    /// The trace, one event per line (§6).
    pub fn trace_text(&self) -> String {
        self.trace.iter().fold(String::new(), |mut text, event| {
            let _ = writeln!(text, "{event}");
            text
        })
    }
}

/// What a host function is given besides its arguments (§6).
#[derive(Debug)]
pub struct HostCx<'r> {
    /// The run's memory.
    pub memory: &'r mut Memory,
    /// How many calls of this host function completed before this one in
    /// this run.
    pub index: u64,
}

type HostFn<'a> = Box<dyn FnMut(&mut HostCx<'_>, &[Val]) -> Result<Option<Val>, Trap> + 'a>;

struct Host<'a> {
    name: Arc<str>,
    call: HostFn<'a>,
}

/// Runs functions of a [`Program`] (§2).
pub struct Interp<'a> {
    program: &'a Program<'a>,
    hosts: Vec<Host<'a>>,
    /// The parameters of the next run.
    pub config: Config,
}

impl fmt::Debug for Interp<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hosts: Vec<&str> = self.hosts.iter().map(|host| &*host.name).collect();
        f.debug_struct("Interp").field("hosts", &hosts).field("config", &self.config).finish()
    }
}

impl<'a> Interp<'a> {
    /// An interpreter for `program` with the default [`Config`] and no
    /// host functions.
    pub fn new(program: &'a Program<'a>) -> Interp<'a> {
        Interp { program, hosts: Vec::new(), config: Config::default() }
    }

    /// The same interpreter with `config`.
    #[must_use]
    pub fn with_config(mut self, config: Config) -> Interp<'a> {
        self.config = config;
        self
    }

    /// Registers `call` as the function named `name`: a call to a function
    /// symbol of that name that has no body in the program runs it (§6).
    /// A second registration of a name replaces the first.
    pub fn host(
        &mut self,
        name: &str,
        call: impl FnMut(&mut HostCx<'_>, &[Val]) -> Result<Option<Val>, Trap> + 'a,
    ) -> &mut Interp<'a> {
        self.hosts.retain(|host| &*host.name != name);
        self.hosts.push(Host { name: name.into(), call: Box::new(call) });
        self
    }

    /// Runs function `entry` with `args` in a fresh memory (§2).
    pub fn run(&mut self, entry: SymId, args: &[Val]) -> Outcome {
        let mut machine = Machine {
            module: self.program.module,
            program: self.program,
            hosts: &mut self.hosts,
            host_of: Vec::new(),
            host_calls: Vec::new(),
            config: self.config,
            mem: Memory::new(self.config.addr_base, self.config.addr_gap, self.config.max_memory),
            addrs: Vec::new(),
            vals: Vec::new(),
            slots: Vec::new(),
            frames: Vec::new(),
            trace: Vec::new(),
            steps: 0,
            max_depth: 0,
            pending: Vec::new(),
            serials: 0,
            builtin_of: Vec::new(),
            block_addrs: HashMap::new(),
            block_at: HashMap::new(),
            eh_tables: HashMap::new(),
            exns: Vec::new(),
            landing: None,
        };
        let result = machine.run(entry, args);
        let stopped_at = match (&result, machine.frames.last()) {
            (Err(_), Some(frame)) => Some((frame.sym, InstId::new(frame.pc))),
            _ => None,
        };
        Outcome {
            result,
            trace: machine.trace,
            steps: machine.steps,
            max_depth: machine.max_depth,
            peak_memory: machine.mem.peak_bytes(),
            stopped_at,
        }
    }
}

/// The kinds of first-class value the interpreter computes with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Void,
    Int(u32),
    Ptr,
    Float,
    Double,
    Agg,
}

impl Class {
    /// Bytes a scalar of this class occupies in memory.
    fn store_len(self) -> usize {
        match self {
            Class::Int(bits) => bits.div_ceil(8) as usize,
            Class::Ptr | Class::Double => 8,
            Class::Float => 4,
            Class::Void | Class::Agg => 0,
        }
    }
}

/// A scalar from its bytes in memory or in an aggregate image.
fn decode(class: Class, bytes: &[u8]) -> Val {
    let mut wide = [0u8; 16];
    wide[..bytes.len()].copy_from_slice(bytes);
    let value = u128::from_le_bytes(wide);
    match class {
        Class::Int(bits) => Val::Int { bits, value },
        Class::Ptr => Val::Ptr(value as u64),
        Class::Float => Val::Float(value as u32),
        Class::Double => Val::Double(value as u64),
        Class::Void | Class::Agg => unreachable!("{class:?} is not a scalar"),
    }
}

/// The bytes of a scalar, or the image of an aggregate, into `out`, which
/// has the value's length.
fn encode(val: &Val, out: &mut [u8]) {
    let wide = match val {
        Val::Int { value, .. } => *value,
        Val::Ptr(addr) => u128::from(*addr),
        Val::Float(bits) => u128::from(*bits),
        Val::Double(bits) => u128::from(*bits),
        Val::Agg(agg) => return out.copy_from_slice(&agg.bytes),
    };
    out.copy_from_slice(&wide.to_le_bytes()[..out.len()]);
}

#[derive(Clone, Copy)]
struct Frame<'a> {
    body: &'a Body,
    sym: SymId,
    /// Where the frame's arguments start in `vals`.
    args: usize,
    /// Where the frame's instruction results start in `vals`.
    insts: usize,
    /// Where the frame's slot addresses start in `slots`.
    slots: usize,
    mark: Mark,
    /// The instruction to execute next.
    pc: u32,
    block: BlockId,
    /// The call that made this frame was marked `noreturn`.
    noreturn: bool,
    /// Which activation this is: frames of a run have distinct serials.
    serial: u64,
}

/// What executing one instruction leads to.
enum Flow {
    /// The instruction's result; go on with the next one.
    Value(Val),
    /// No result; go on with the next one.
    Next,
    Jump(BlockId),
    Return(Option<Val>),
    /// A frame was pushed.
    Called,
    /// An exception transferred control to a landing pad (§5.9).
    Unwound,
}

/// What one step of the run loop leads to.
enum Step {
    Running,
    /// The entry function returned this.
    Done(Option<Val>),
}

struct Machine<'r, 'a> {
    module: &'a Module,
    program: &'a Program<'a>,
    hosts: &'r mut [Host<'a>],
    /// By `SymId`: one more than an index into `hosts`, or 0.
    host_of: Vec<u32>,
    /// Completed calls per host.
    host_calls: Vec<u64>,
    config: Config,
    mem: Memory,
    /// By `SymId`: the symbol's address, or 0 when it has none.
    addrs: Vec<u64>,
    /// Arguments and instruction results of every live frame; `None` is
    /// "not computed yet".
    vals: Vec<Option<Val>>,
    slots: Vec<u64>,
    frames: Vec<Frame<'a>>,
    trace: Vec<Event>,
    steps: u64,
    max_depth: u32,
    /// The values of a block's `phi`s while an edge is taken.
    pending: Vec<Val>,
    /// Frames pushed so far: the next frame's serial.
    serials: u64,
    /// By `SymId`: which function of the model (§5.9) a declaration is.
    builtin_of: Vec<Builtin>,
    /// The address of each `blockaddress` of the module (§5.5).
    block_addrs: HashMap<(SymId, BlockId), u64>,
    block_at: HashMap<u64, (SymId, BlockId)>,
    /// The type-id tables of the functions that unwound (§5.9).
    eh_tables: HashMap<SymId, Rc<EhTable>>,
    /// Every exception of the run, in the order they were thrown.
    exns: Vec<Exn>,
    /// The exception and selector of the landing pad being entered.
    landing: Option<(u64, i32)>,
}

/// The functions of the exception model (§5.9) a declaration may be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Builtin {
    None,
    Throw,
    Catch,
}

/// Where an exception is (§5.9).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ExnState {
    /// Being unwound: no code runs.
    Raised,
    /// A landing pad received it: it may be resumed or caught.
    Landed,
    /// Caught by `hod_catch`.
    Done,
}

#[derive(Clone, Debug)]
struct Exn {
    /// Its address: an identity, not memory.
    addr: u64,
    /// The address of its type-information object.
    ty: u64,
    payload: Val,
    /// The serial of the frame the search phase chose, and the call its
    /// handler is at.
    handler: u64,
    handler_pc: u32,
    /// It landed in the handler (not a cleanup): `hod_catch` may take it.
    landed_handler: bool,
    state: ExnState,
}

/// One landing-pad clause with the function's type ids resolved.
#[derive(Clone, Debug)]
enum EhClause {
    /// Matches this type, or every type when it is 0; selector `id`.
    Catch { ty: u64, id: i32 },
    /// Matches every type not listed; selector `id`, which is negative.
    Filter { tys: Vec<u64>, id: i32 },
}

/// A function's view of the personality's tables (§5.9).
#[derive(Debug, Default)]
struct EhTable {
    /// Type-information addresses (0: catch-all) in order of first
    /// mention; a type's id is its index + 1.
    types: Vec<u64>,
    /// The clauses of each landing pad, by the `landingpad` instruction.
    pads: HashMap<InstId, Vec<EhClause>>,
}

/// What the personality does with an exception at one landing pad.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PadMatch {
    /// A clause matches: the pad is a handler, with this selector.
    Handler(i32),
    /// No clause matches and the pad is a cleanup: selector 0.
    Cleanup,
    /// The pad is not entered.
    Nothing,
}

impl<'a> Machine<'_, 'a> {
    fn run(&mut self, entry: SymId, args: &[Val]) -> Result<Option<Val>, Stop> {
        if self.module.types.size_of(TypeId::PTR) != 8 {
            return unsupported("target:pointer-width");
        }
        if self.module.types.data_layout().big_endian {
            return unsupported("target:big-endian");
        }
        self.load_module()?;
        let Some(body) = self.program.body(entry) else {
            return invalid(format!("the entry function {entry:?} has no body in the program"));
        };
        let params = body.arg_types(self.module);
        if params.len() != args.len() {
            return invalid(format!("{} arguments for {} parameters", args.len(), params.len()));
        }
        for (index, (arg, &param)) in args.iter().zip(params).enumerate() {
            if !self.fits(arg, param)? {
                return invalid(format!("argument {index} ({arg}) is not of the parameter's type"));
            }
        }
        self.push_frame(entry, body, args.to_vec(), false)?;
        loop {
            match self.step() {
                Ok(Step::Done(result)) => return Ok(result),
                Ok(Step::Running) => {}
                // §4: an access to a declared variable is not refused for
                // the program's sake: what the variable holds is unknown.
                Err(Stop::Trap(Trap::OutOfBounds))
                    if let Some(sym) = self.mem.take_extern_hit() =>
                {
                    let name = String::from_utf8_lossy(self.module.symbol(sym).name()).into_owned();
                    return unsupported(format!("extern:{name}"));
                }
                Err(stop) => return Err(stop),
            }
            self.mem.take_extern_hit();
        }
    }

    // ---- the module ----

    /// Gives every function and every defined variable an address and
    /// writes the variables' initial images (§4).
    fn load_module(&mut self) -> Result<(), Stop> {
        let module = self.module;
        let count = module.symbol_count();
        self.addrs = vec![0; count];
        self.host_of = vec![0; count];
        self.builtin_of = vec![Builtin::None; count];
        self.host_calls = vec![0; self.hosts.len()];
        for (index, host) in self.hosts.iter().enumerate() {
            if let Some(sym) = module.find_symbol(host.name.as_bytes()) {
                self.host_of[sym.index()] = index as u32 + 1;
            }
        }
        let mut image = Vec::new();
        let mut fixups = Vec::new();
        let mut patches = Vec::new();
        let mut spans = Vec::new();
        let mut declared = Vec::new();
        for symbol in module.symbols() {
            let sym = symbol.id();
            match symbol.kind() {
                SymbolKind::Function { .. } => {
                    let builtin = match symbol.name() {
                        name if name == eh::THROW.as_bytes() => Builtin::Throw,
                        name if name == eh::CATCH.as_bytes() => Builtin::Catch,
                        _ => Builtin::None,
                    };
                    self.builtin_of[sym.index()] = builtin;
                    let defined = self.program.body(sym).is_some()
                        || self.host_of[sym.index()] != 0
                        || builtin != Builtin::None;
                    if defined || symbol.linkage() != Linkage::ExternWeak {
                        self.addrs[sym.index()] = self.mem.alloc_function(sym)?;
                    }
                }
                SymbolKind::Variable { constant, tls: TlsModel::None, init: Some(init) } => {
                    let ty = symbol.ty();
                    let size = module.types.alloc_size_of(ty);
                    if size > self.config.max_memory {
                        return Err(Trap::Memory.into());
                    }
                    image.clear();
                    fixups.clear();
                    if let Err(error) =
                        module.consts.flatten(&module.types, init, &mut image, &mut fixups)
                    {
                        return match error.kind {
                            IrErrorKind::UnsupportedInitializer => {
                                unsupported("global:initializer")
                            }
                            _ => invalid(format!("the initializer of {sym:?}: {error}")),
                        };
                    }
                    let align = symbol.align().unwrap_or_else(|| module.types.align_of(ty));
                    let kind = if constant { AllocKind::Constant } else { AllocKind::Data };
                    let base = self.mem.alloc(size, align, kind)?;
                    self.addrs[sym.index()] = base;
                    if size != 0 {
                        // A fresh allocation: the access cannot fail.
                        let at = self.mem.check(base, size, 1, false)?;
                        self.mem.put(at, &image);
                        // Padding is in the image as zeros, but no
                        // compilation promises to keep it: unwritten.
                        spans.clear();
                        self.spans(ty, 0, &mut spans);
                        if spans != [(0, size)] {
                            self.mem.set_written(at, size as usize, false);
                            for &(offset, len) in &spans {
                                self.mem.set_written(at + offset as usize, len as usize, true);
                            }
                        }
                        patches.extend(fixups.iter().map(|fixup| (at, *fixup)));
                    }
                }
                SymbolKind::Variable { tls: TlsModel::None, init: None, .. } => declared.push(sym),
                _ => {}
            }
        }
        // After everything else, so that these do not move the addresses
        // of programs without them.
        for sym in declared {
            self.addrs[sym.index()] = self.mem.alloc_extern(sym)?;
        }
        for index in 0..module.consts.len() {
            if let Const::BlockAddr { func, block } = module.consts.get(ConstId::new(index as u32))
            {
                let addr = self.mem.reserve()?;
                self.block_addrs.insert((func, block), addr);
                self.block_at.insert(addr, (func, block));
            }
        }
        for (at, fixup) in patches {
            let value = self.fixup_value(fixup.target)?.wrapping_add(fixup.addend as u64);
            let bytes = value.to_le_bytes();
            self.mem.put(at + fixup.offset as usize, &bytes[..usize::from(fixup.size).min(8)]);
        }
        Ok(())
    }

    fn fixup_value(&self, target: FixupTarget) -> Result<u64, Stop> {
        match target {
            FixupTarget::Sym(sym) | FixupTarget::Plt(sym) => self.sym_addr(sym),
            FixupTarget::Diff { .. } | FixupTarget::PltDiff { .. } => unsupported("const:symdiff"),
            FixupTarget::Block { func, block } => self.block_addr(func, block),
        }
    }

    /// The address of `blockaddress(func, block)` (§5.5).
    fn block_addr(&self, func: SymId, block: BlockId) -> Result<u64, Stop> {
        match self.block_addrs.get(&(func, block)) {
            Some(&addr) => Ok(addr),
            None => invalid(format!("blockaddress({func:?}, {block:?}) is not a constant")),
        }
    }

    /// The address of a symbol, or why it has none here.
    fn sym_addr(&self, sym: SymId) -> Result<u64, Stop> {
        match self.addrs.get(sym.index()) {
            Some(&addr) if addr != 0 => return Ok(addr),
            Some(_) => {}
            None => return invalid(format!("{sym:?} is not a symbol of the module")),
        }
        let symbol = self.module.symbol(sym);
        match symbol.kind() {
            SymbolKind::Unresolved => invalid(format!("{sym:?} was never described")),
            SymbolKind::Function { .. } => unsupported("global:extern-weak"),
            SymbolKind::Variable { tls, init, .. } => {
                if tls != TlsModel::None {
                    unsupported("global:thread-local")
                } else if init.is_none() {
                    unsupported(format!("extern:{}", String::from_utf8_lossy(symbol.name())))
                } else {
                    invalid(format!("{sym:?} has no address"))
                }
            }
            SymbolKind::Alias { .. } => unsupported("global:alias"),
            SymbolKind::IFunc { .. } => unsupported("global:ifunc"),
        }
    }

    // ---- types and constants ----

    fn class(&self, ty: TypeId) -> Result<Class, Stop> {
        let types = &self.module.types;
        if ty.index() >= types.len() {
            return invalid(format!("{ty:?} is not a type of the module"));
        }
        Ok(match types.kind(ty) {
            TypeKind::Void => Class::Void,
            TypeKind::Int(bits) if bits <= MAX_INT_BITS => Class::Int(bits),
            TypeKind::Int(_) => return unsupported("type:int-over-128"),
            TypeKind::Ptr { .. } => Class::Ptr,
            TypeKind::Float => Class::Float,
            TypeKind::Double => Class::Double,
            TypeKind::Array { .. } | TypeKind::Struct { .. } => Class::Agg,
            TypeKind::Half => return unsupported("type:half"),
            TypeKind::BFloat => return unsupported("type:bfloat"),
            TypeKind::Fp128 => return unsupported("type:fp128"),
            TypeKind::X86Fp80 => return unsupported("type:x86_fp80"),
            TypeKind::Vector { .. } => return unsupported("type:vector"),
            TypeKind::ScalableVector { .. } => return unsupported("type:scalable-vector"),
            TypeKind::Opaque(_) => return unsupported("type:opaque"),
            TypeKind::Function { .. } => return unsupported("type:function"),
            TypeKind::Label => return unsupported("type:label"),
            TypeKind::Metadata => return unsupported("type:metadata"),
            TypeKind::Token => return unsupported("type:token"),
            TypeKind::TargetExt { .. } => return unsupported("type:target-ext"),
        })
    }

    /// Whether `val` is a value of type `ty`.
    fn fits(&self, val: &Val, ty: TypeId) -> Result<bool, Stop> {
        Ok(match (self.class(ty)?, val) {
            (Class::Int(bits), Val::Int { bits: have, .. }) => bits == *have,
            (Class::Ptr, Val::Ptr(_))
            | (Class::Float, Val::Float(_))
            | (Class::Double, Val::Double(_)) => true,
            (Class::Agg, Val::Agg(agg)) => agg.ty == ty,
            _ => false,
        })
    }

    /// The size of an aggregate image, refused when it alone is over the
    /// memory limit.
    fn image_len(&self, ty: TypeId) -> Result<usize, Stop> {
        let size = self.module.types.alloc_size_of(ty);
        if size > self.config.max_memory {
            return Err(Trap::Memory.into());
        }
        Ok(size as usize)
    }

    /// The byte ranges of an object of type `ty` that hold its scalars:
    /// everything but padding. Unlike [`Machine::leaves`] this takes any
    /// sized type.
    fn spans(&self, ty: TypeId, offset: u64, out: &mut Vec<(u64, u64)>) {
        let types = &self.module.types;
        match types.kind(ty) {
            TypeKind::Struct { fields, .. } => {
                let offsets = types.field_offsets(ty).unwrap_or(&[]);
                for (&field, &at) in fields.iter().zip(offsets) {
                    self.spans(field, offset + at, out);
                }
            }
            TypeKind::Array { elem, len } => {
                let stride = types.alloc_size_of(elem);
                let nested =
                    matches!(types.kind(elem), TypeKind::Struct { .. } | TypeKind::Array { .. });
                if !nested && types.size_of(elem) == stride {
                    out.push((offset, len * stride));
                } else if stride != 0 {
                    for index in 0..len {
                        self.spans(elem, offset + index * stride, out);
                    }
                }
            }
            _ => out.push((offset, types.size_of(ty))),
        }
    }

    /// The scalars of aggregate type `ty` as `(byte offset, class)`.
    fn leaves(&self, ty: TypeId, offset: u64, out: &mut Vec<(usize, Class)>) -> Result<(), Stop> {
        let types = &self.module.types;
        match types.kind(ty) {
            TypeKind::Struct { fields, .. } => {
                let offsets = types.field_offsets(ty).unwrap_or(&[]);
                for (&field, &at) in fields.iter().zip(offsets) {
                    self.leaves(field, offset + at, out)?;
                }
            }
            TypeKind::Array { elem, len } => {
                let stride = types.alloc_size_of(elem);
                if stride != 0 {
                    for index in 0..len {
                        self.leaves(elem, offset + index * stride, out)?;
                    }
                }
            }
            _ => out.push((offset as usize, self.class(ty)?)),
        }
        Ok(())
    }

    fn zero(&self, ty: TypeId) -> Result<Val, Stop> {
        Ok(match self.class(ty)? {
            Class::Int(bits) => Val::Int { bits, value: 0 },
            Class::Ptr => Val::Ptr(0),
            Class::Float => Val::Float(0),
            Class::Double => Val::Double(0),
            Class::Agg => {
                self.leaves(ty, 0, &mut Vec::new())?;
                Val::Agg(Agg { ty, bytes: vec![0; self.image_len(ty)?].into() })
            }
            Class::Void => return invalid("a value of type void"),
        })
    }

    fn constant(&self, id: ConstId) -> Result<Val, Stop> {
        let module = self.module;
        if id.index() >= module.consts.len() {
            return invalid(format!("{id:?} is not a constant of the module"));
        }
        Ok(match module.consts.get(id) {
            Const::Int { ty, value } => match self.class(ty)? {
                Class::Int(bits) => Val::Int { bits, value: u128::from(value) },
                _ => return invalid(format!("{id:?} is an integer of a type that is not")),
            },
            Const::WideInt { ty, words } => match self.class(ty)? {
                Class::Int(bits) => {
                    let low = words.first().copied().unwrap_or(0);
                    let high = words.get(1).copied().unwrap_or(0);
                    Val::Int { bits, value: u128::from(low) | (u128::from(high) << 64) }
                }
                _ => return invalid(format!("{id:?} is an integer of a type that is not")),
            },
            Const::Float { ty, bits } => match self.class(ty)? {
                Class::Float => Val::Float(bits as u32),
                Class::Double => Val::Double(bits),
                _ => return invalid(format!("{id:?} is a float of a type that is not")),
            },
            Const::WideFloat { ty, .. } => {
                self.class(ty)?;
                return invalid(format!("{id:?} is a wide float of a narrow type"));
            }
            Const::Null(_) => Val::Ptr(0),
            // `spec/ir.md` §4.1: `undef` and `poison` are zero.
            Const::Zero(ty) | Const::Undef(ty) | Const::Poison(ty) => self.zero(ty)?,
            Const::NoneToken => return unsupported("type:token"),
            Const::Aggregate { ty, .. } | Const::Splat { ty, .. } | Const::Bytes { ty, .. } => {
                self.class(ty)?;
                self.leaves(ty, 0, &mut Vec::new())?;
                self.image_len(ty)?;
                let mut image = Vec::new();
                let mut fixups = Vec::new();
                if let Err(error) =
                    module.consts.flatten(&module.types, id, &mut image, &mut fixups)
                {
                    return match error.kind {
                        IrErrorKind::UnsupportedInitializer => unsupported("const:aggregate"),
                        _ => invalid(format!("{id:?}: {error}")),
                    };
                }
                for fixup in fixups {
                    let value = self.fixup_value(fixup.target)?.wrapping_add(fixup.addend as u64);
                    let at = fixup.offset as usize;
                    let len = usize::from(fixup.size).min(8);
                    image[at..at + len].copy_from_slice(&value.to_le_bytes()[..len]);
                }
                Val::Agg(Agg { ty, bytes: image.into() })
            }
            Const::Addr { sym, addend } => {
                Val::Ptr(self.sym_addr(sym)?.wrapping_add(addend as u64))
            }
            Const::DsoLocalEquivalent(sym) | Const::NoCfi(sym) => Val::Ptr(self.sym_addr(sym)?),
            Const::AddrOfInt { value } => Val::Ptr(value),
            Const::IntOfAddr { ty, addr } => match (self.class(ty)?, self.constant(addr)?) {
                (Class::Int(bits), Val::Ptr(addr)) => Val::int(bits, u128::from(addr)),
                _ => return invalid(format!("{id:?} is not an integer of an address")),
            },
            Const::BlockAddr { func, block } => Val::Ptr(self.block_addr(func, block)?),
            Const::SymDiff { .. } => return unsupported("const:symdiff"),
            Const::Expr { op, .. } => return unsupported(format!("const-expr:{}", op.name())),
        })
    }

    // ---- operands ----

    fn get(&self, f: &Frame<'a>, value: Value) -> Result<Val, Stop> {
        match value.kind() {
            ValueKind::Inst(id) => {
                if id.index() >= f.body.inst_count() {
                    return invalid(format!("{id:?} is not an instruction of the body"));
                }
                match &self.vals[f.insts + id.index()] {
                    Some(val) => Ok(val.clone()),
                    None => Err(Trap::UndefinedValue.into()),
                }
            }
            ValueKind::Arg(index) => {
                if index >= f.body.arg_count() {
                    return invalid(format!("the function has no argument {index}"));
                }
                match &self.vals[f.args + index as usize] {
                    Some(val) => Ok(val.clone()),
                    None => invalid(format!("argument {index} was not passed")),
                }
            }
            ValueKind::Const(id) => self.constant(id),
            ValueKind::Sym(sym) => Ok(Val::Ptr(self.sym_addr(sym)?)),
            ValueKind::Slot(slot) => match self.slots.get(f.slots + slot.index()) {
                Some(&addr) if slot.index() < f.body.slots().len() => Ok(Val::Ptr(addr)),
                _ => invalid(format!("{slot:?} is not a slot of the body")),
            },
            ValueKind::Asm(_) => unsupported("call:inline-asm"),
            ValueKind::None => invalid("an operand is missing"),
        }
    }

    fn int(&self, f: &Frame<'a>, value: Value) -> Result<(u32, u128), Stop> {
        match self.get(f, value)? {
            Val::Int { bits, value } => Ok((bits, value)),
            other => invalid(format!("{other} where an integer is required")),
        }
    }

    fn ptr(&self, f: &Frame<'a>, value: Value) -> Result<u64, Stop> {
        match self.get(f, value)? {
            Val::Ptr(addr) => Ok(addr),
            other => invalid(format!("{other} where a pointer is required")),
        }
    }

    /// A computed float: a NaN stops the run unless the policy lets the
    /// host's NaN through (§5.1).
    fn float(&self, value: f32) -> Result<Val, Stop> {
        if value.is_nan() && self.config.nan == NanPolicy::Trap {
            return Err(Trap::NanResult.into());
        }
        Ok(Val::Float(value.to_bits()))
    }

    /// As [`Machine::float`], for a `double`.
    fn double(&self, value: f64) -> Result<Val, Stop> {
        if value.is_nan() && self.config.nan == NanPolicy::Trap {
            return Err(Trap::NanResult.into());
        }
        Ok(Val::Double(value.to_bits()))
    }

    // ---- memory ----

    fn load(&self, ty: TypeId, addr: u64, align: u64) -> Result<Val, Stop> {
        let class = self.class(ty)?;
        match class {
            Class::Void => invalid("a load of type void"),
            Class::Agg => {
                let mut leaves = Vec::new();
                self.leaves(ty, 0, &mut leaves)?;
                let mut image = vec![0u8; self.image_len(ty)?];
                let size = self.module.types.size_of(ty);
                if size != 0 {
                    let at = self.mem.check(addr, size, align, false)?;
                    for (offset, leaf) in leaves {
                        let len = leaf.store_len();
                        if !self.mem.is_written(at + offset, len) {
                            return Err(Trap::UninitializedLoad.into());
                        }
                        let bytes = self.mem.raw(at + offset, len);
                        Self::check_padding(leaf, bytes)?;
                        image[offset..offset + len].copy_from_slice(bytes);
                    }
                }
                Ok(Val::Agg(Agg { ty, bytes: image.into() }))
            }
            scalar => {
                let bytes = self.mem.load(addr, scalar.store_len() as u64, align)?;
                Self::check_padding(scalar, bytes)?;
                Ok(decode(scalar, bytes))
            }
        }
    }

    /// An integer whose width is not a whole number of bytes must have
    /// zero bits above its width in memory (`spec/codegen.md` §2.2).
    fn check_padding(class: Class, bytes: &[u8]) -> Result<(), Trap> {
        if let Class::Int(bits) = class
            && let Some(&top) = bytes.last()
            && !bits.is_multiple_of(8)
            && top >> (bits % 8) != 0
        {
            return Err(Trap::NonZeroPadding);
        }
        Ok(())
    }

    fn store(&mut self, val: &Val, addr: u64, align: u64) -> Result<(), Stop> {
        match val {
            Val::Agg(agg) => {
                let mut leaves = Vec::new();
                self.leaves(agg.ty, 0, &mut leaves)?;
                let size = self.module.types.size_of(agg.ty);
                if size != 0 {
                    let at = self.mem.check(addr, size, align, true)?;
                    // What lies between the scalars is not written by
                    // every compilation: afterwards it is unwritten.
                    self.mem.set_written(at, size as usize, false);
                    for (offset, leaf) in leaves {
                        let len = leaf.store_len();
                        self.mem.put(at + offset, &agg.bytes[offset..offset + len]);
                    }
                }
            }
            Val::Int { bits, .. } => {
                let mut bytes = [0u8; 16];
                let len = bits.div_ceil(8) as usize;
                encode(val, &mut bytes[..len]);
                self.mem.store(addr, &bytes[..len], align)?;
            }
            Val::Ptr(_) | Val::Double(_) => {
                let mut bytes = [0u8; 8];
                encode(val, &mut bytes);
                self.mem.store(addr, &bytes, align)?;
            }
            Val::Float(_) => {
                let mut bytes = [0u8; 4];
                encode(val, &mut bytes);
                self.mem.store(addr, &bytes, align)?;
            }
        }
        Ok(())
    }

    // ---- frames ----

    fn push_frame(
        &mut self,
        sym: SymId,
        body: &'a Body,
        args: Vec<Val>,
        noreturn: bool,
    ) -> Result<(), Stop> {
        if self.frames.len() >= self.config.max_call_depth as usize {
            return Err(Trap::CallDepth.into());
        }
        if body.block_count() == 0 || body.entry().index() >= body.block_count() {
            return invalid(format!("the body of {sym:?} has no entry block"));
        }
        let mark = self.mem.mark();
        let slots = self.slots.len();
        for slot in body.slots() {
            match self.mem.alloc(slot.size(), slot.align(), AllocKind::Stack) {
                Ok(addr) => self.slots.push(addr),
                Err(trap) => {
                    self.mem.release(mark);
                    self.slots.truncate(slots);
                    return Err(trap.into());
                }
            }
        }
        let base = self.vals.len();
        self.vals.extend(args.into_iter().map(Some));
        let insts = self.vals.len();
        self.vals.resize(insts + body.inst_count(), None);
        let entry = body.block(body.entry());
        let first = entry.inst_range().start.saturating_add(entry.phis().len() as u32);
        self.frames.push(Frame {
            body,
            sym,
            args: base,
            insts,
            slots,
            mark,
            pc: first,
            block: entry.id(),
            noreturn,
            serial: self.serials,
        });
        self.serials += 1;
        self.max_depth = self.max_depth.max(self.frames.len() as u32);
        Ok(())
    }

    fn tick(&mut self, count: u64) -> Result<(), Stop> {
        self.steps += count;
        if self.steps > self.config.fuel {
            self.steps = self.config.fuel;
            return Err(Trap::Fuel.into());
        }
        Ok(())
    }

    /// Executes one instruction.
    fn step(&mut self) -> Result<Step, Stop> {
        let Some(&f) = self.frames.last() else {
            return invalid("no frame");
        };
        if f.pc as usize >= f.body.inst_count() {
            return invalid("control ran past the last instruction");
        }
        self.tick(1)?;
        let inst = f.body.inst(InstId::new(f.pc));
        match self.execute(&f, inst)? {
            Flow::Value(val) => {
                self.vals[f.insts + f.pc as usize] = Some(val);
                self.advance();
            }
            Flow::Next => self.advance(),
            Flow::Jump(to) => self.jump(&f, to)?,
            Flow::Called | Flow::Unwound => {}
            Flow::Return(val) => {
                self.pop_frame();
                let Some(&caller) = self.frames.last() else {
                    return Ok(Step::Done(val));
                };
                if f.noreturn {
                    return Err(Trap::NoReturnReturned.into());
                }
                if let Some(val) = val {
                    self.vals[caller.insts + caller.pc as usize] = Some(val);
                }
                // An `invoke` continues at its normal destination.
                match caller.body.inst(InstId::new(caller.pc)).call().and_then(|c| c.normal_dest())
                {
                    Some(normal) => self.jump(&caller, normal)?,
                    None => self.advance(),
                }
            }
        }
        Ok(Step::Running)
    }

    /// Ends the innermost frame: its memory dies, its values are gone.
    fn pop_frame(&mut self) {
        if let Some(f) = self.frames.pop() {
            self.mem.release(f.mark);
            self.vals.truncate(f.args);
            self.slots.truncate(f.slots);
        }
    }

    fn advance(&mut self) {
        if let Some(frame) = self.frames.last_mut() {
            frame.pc += 1;
        }
    }

    /// Takes the edge from the frame's current block to `to`: the `phi`s
    /// of `to` all read their inputs before any of them is written (§5.5).
    fn jump(&mut self, f: &Frame<'a>, to: BlockId) -> Result<(), Stop> {
        if to.index() >= f.body.block_count() {
            return invalid(format!("{to:?} is not a block of the body"));
        }
        let block = f.body.block(to);
        let mut pending = std::mem::take(&mut self.pending);
        pending.clear();
        for phi in block.phis() {
            let InstKind::Phi(list) = phi.kind() else {
                return invalid(format!("{:?} is not a phi", phi.id()));
            };
            let Some((value, _)) = list.incoming().find(|&(_, from)| from == f.block) else {
                return invalid(format!("{:?} has no entry for {:?}", phi.id(), f.block));
            };
            pending.push(self.get(f, value)?);
        }
        self.tick(pending.len() as u64)?;
        let first = block.inst_range().start;
        for (index, val) in pending.drain(..).enumerate() {
            self.vals[f.insts + first as usize + index] = Some(val);
        }
        let count = block.phis().len() as u32;
        self.pending = pending;
        if let Some(frame) = self.frames.last_mut() {
            frame.block = to;
            frame.pc = first.saturating_add(count);
        }
        Ok(())
    }

    // ---- instructions ----

    #[allow(clippy::too_many_lines)]
    fn execute(&mut self, f: &Frame<'a>, inst: Inst<'a>) -> Result<Flow, Stop> {
        let op = inst.opcode();
        let ty = inst.ty();
        Ok(Flow::Value(match inst.kind() {
            InstKind::Binary { lhs, rhs } if op.is_int_binary() => {
                let ((bits, a), (other, b)) = (self.int(f, lhs)?, self.int(f, rhs)?);
                if bits != other {
                    return invalid("operands of two widths");
                }
                Val::Int { bits, value: ops::int_binary(op, bits, a, b)? }
            }
            InstKind::Binary { lhs, rhs } => match (self.get(f, lhs)?, self.get(f, rhs)?) {
                (Val::Float(a), Val::Float(b)) => {
                    self.float(ops::f32_binary(op, f32::from_bits(a), f32::from_bits(b)))?
                }
                (Val::Double(a), Val::Double(b)) => {
                    self.double(ops::f64_binary(op, f64::from_bits(a), f64::from_bits(b)))?
                }
                _ => {
                    self.class(ty)?;
                    return invalid("float arithmetic on something else");
                }
            },
            InstKind::Unary { arg } => match (op, self.get(f, arg)?) {
                (Opcode::FNeg, Val::Float(bits)) => Val::Float(bits ^ (1 << 31)),
                (Opcode::FNeg, Val::Double(bits)) => Val::Double(bits ^ (1 << 63)),
                (Opcode::FNeg, _) => {
                    self.class(ty)?;
                    return invalid("fneg of something that is not a float");
                }
                // `freeze` is a copy (`spec/codegen.md` §4.7).
                (_, val) => val,
            },
            InstKind::Cast { arg } => self.cast(op, ty, self.get(f, arg)?)?,
            InstKind::ICmp { pred, lhs, rhs } => match (self.get(f, lhs)?, self.get(f, rhs)?) {
                (Val::Int { bits, value: a }, Val::Int { bits: other, value: b })
                    if bits == other =>
                {
                    Val::i1(ops::icmp(pred, bits, a, b))
                }
                (Val::Ptr(a), Val::Ptr(b)) => {
                    Val::i1(ops::icmp(pred, 64, u128::from(a), u128::from(b)))
                }
                _ => {
                    self.class(ty)?;
                    return invalid("icmp of operands that are not two integers or pointers");
                }
            },
            InstKind::FCmp { pred, lhs, rhs } => match (self.get(f, lhs)?, self.get(f, rhs)?) {
                (Val::Float(a), Val::Float(b)) => Val::i1(ops::fcmp(
                    pred,
                    f64::from(f32::from_bits(a)),
                    f64::from(f32::from_bits(b)),
                )),
                (Val::Double(a), Val::Double(b)) => {
                    Val::i1(ops::fcmp(pred, f64::from_bits(a), f64::from_bits(b)))
                }
                _ => {
                    self.class(ty)?;
                    return invalid("fcmp of operands that are not two floats");
                }
            },
            InstKind::Load { ptr, mem } => self.load(ty, self.ptr(f, ptr)?, mem.align())?,
            InstKind::Store { value, ptr, mem } => {
                let val = self.get(f, value)?;
                self.store(&val, self.ptr(f, ptr)?, mem.align())?;
                return Ok(Flow::Next);
            }
            InstKind::Alloca { count, elem_size, align } => {
                let (_, count) = self.int(f, count)?;
                let size = count
                    .checked_mul(u128::from(elem_size))
                    .and_then(|size| u64::try_from(size).ok())
                    .ok_or(Trap::Memory)?;
                Val::Ptr(self.mem.alloc(size, align, AllocKind::Stack)?)
            }
            InstKind::Fence { .. } => return Ok(Flow::Next),
            InstKind::CmpXchg { ptr, expected, replacement, mem, .. } => {
                let addr = self.ptr(f, ptr)?;
                let (expected, replacement) = (self.get(f, expected)?, self.get(f, replacement)?);
                let Some(&[value_ty, flag_ty]) = self.module.types.struct_fields(ty) else {
                    return invalid("the result of cmpxchg is not a pair");
                };
                let old = self.load(value_ty, addr, mem.align())?;
                let same = old == expected;
                if same {
                    self.store(&replacement, addr, mem.align())?;
                }
                self.pair(ty, &old, &Val::i1(same), flag_ty)?
            }
            InstKind::AtomicRmw { op: rmw, ptr, value, mem } => {
                let addr = self.ptr(f, ptr)?;
                let operand = self.get(f, value)?;
                let old = self.load(ty, addr, mem.align())?;
                let new = self.rmw(rmw, &old, operand)?;
                self.store(&new, addr, mem.align())?;
                old
            }
            InstKind::PtrAdd(add) => {
                self.class(ty)?;
                let mut addr = self.ptr(f, add.base)?.wrapping_add(add.offset as u64);
                for (index, scale) in add.indices() {
                    let (bits, value) = self.int(f, index)?;
                    addr = addr.wrapping_add((sext(bits, value) as u64).wrapping_mul(scale));
                }
                Val::Ptr(addr)
            }
            InstKind::ExtractValue { agg, offset } => {
                let Val::Agg(agg) = self.get(f, agg)? else {
                    return invalid("extractvalue from something that is not an aggregate");
                };
                let class = self.class(ty)?;
                let len = match class {
                    Class::Agg => self.image_len(ty)?,
                    scalar => scalar.store_len(),
                };
                let bytes = usize::try_from(offset)
                    .ok()
                    .and_then(|at| agg.bytes.get(at..)?.get(..len))
                    .ok_or_else(|| Stop::Invalid("extractvalue outside the aggregate".into()))?;
                match class {
                    Class::Agg => Val::Agg(Agg { ty, bytes: bytes.into() }),
                    Class::Void => return invalid("extractvalue of type void"),
                    scalar => decode(scalar, bytes),
                }
            }
            InstKind::InsertValue { agg, value, offset } => {
                let (Val::Agg(mut agg), field) = (self.get(f, agg)?, self.get(f, value)?) else {
                    return invalid("insertvalue into something that is not an aggregate");
                };
                let len = match &field {
                    Val::Int { bits, .. } => Class::Int(*bits).store_len(),
                    Val::Ptr(_) | Val::Double(_) => 8,
                    Val::Float(_) => 4,
                    Val::Agg(inner) => inner.bytes.len(),
                };
                let at = usize::try_from(offset).unwrap_or(usize::MAX);
                let Some(bytes) = agg.bytes.get_mut(at..).and_then(|rest| rest.get_mut(..len))
                else {
                    return invalid("insertvalue outside the aggregate");
                };
                encode(&field, bytes);
                Val::Agg(agg)
            }
            InstKind::ExtractElement { .. }
            | InstKind::InsertElement { .. }
            | InstKind::ShuffleVector { .. } => return unsupported("type:vector"),
            InstKind::Phi(_) => return invalid("a phi was reached without taking an edge"),
            InstKind::Select { cond, if_true, if_false } => {
                self.class(ty)?;
                let (_, cond) = self.int(f, cond)?;
                self.get(f, if cond != 0 { if_true } else { if_false })?
            }
            InstKind::VaArg { .. } => return unsupported("inst:va_arg"),
            InstKind::Call(call) if op == Opcode::Call => return self.call(f, call, ty),
            InstKind::Call(call) if op == Opcode::Invoke => {
                let Some(normal) = call.normal_dest() else {
                    return invalid("an invoke without a normal destination");
                };
                return Ok(match self.call(f, call, ty)? {
                    Flow::Value(val) => {
                        self.vals[f.insts + f.pc as usize] = Some(val);
                        Flow::Jump(normal)
                    }
                    Flow::Next => Flow::Jump(normal),
                    other => other,
                });
            }
            InstKind::LandingPad(_) => {
                let Some((exn, selector)) = self.landing.take() else {
                    return invalid("a landingpad reached other than by unwinding");
                };
                let Some(&[_, sel_ty]) = self.module.types.struct_fields(ty) else {
                    return invalid("a landingpad whose type is not a pair");
                };
                if sel_ty != TypeId::I32 {
                    return invalid("a landingpad whose selector is not an i32");
                }
                self.pair_of(ty, &Val::Ptr(exn), &Val::i32(selector as u32))?
            }
            InstKind::Resume { value } => {
                let Val::Agg(agg) = self.get(f, value)? else {
                    return invalid("resume of something that is not a pair");
                };
                let mut word = [0u8; 8];
                match agg.bytes.get(..8) {
                    Some(bytes) => word.copy_from_slice(bytes),
                    None => return invalid("resume of a pair without a pointer"),
                }
                self.resume(u64::from_le_bytes(word))?;
                return Ok(Flow::Unwound);
            }
            InstKind::IndirectBr { addr, targets } => {
                let addr = self.ptr(f, addr)?;
                return match self.block_at.get(&addr) {
                    Some(&(func, block))
                        if func == f.sym && targets.into_iter().any(|t| t == block) =>
                    {
                        Ok(Flow::Jump(block))
                    }
                    _ => Err(Trap::IndirectBrTarget.into()),
                };
            }
            InstKind::Call(_) | InstKind::Funclet => {
                return unsupported(format!("inst:{}", op.name()));
            }
            InstKind::Br { target } => return Ok(Flow::Jump(target)),
            InstKind::CondBr { cond, if_true, if_false } => {
                let (_, cond) = self.int(f, cond)?;
                return Ok(Flow::Jump(if cond != 0 { if_true } else { if_false }));
            }
            InstKind::Switch(switch) => {
                let (_, cond) = self.int(f, switch.cond)?;
                for (case, target) in switch.cases() {
                    if self.constant(case)?.as_int() == Some(cond) {
                        return Ok(Flow::Jump(target));
                    }
                }
                return Ok(Flow::Jump(switch.default));
            }
            InstKind::Ret { value } => {
                return Ok(Flow::Return(match value {
                    Some(value) => Some(self.get(f, value)?),
                    None => None,
                }));
            }
            InstKind::Unreachable => return Err(Trap::Unreachable.into()),
        }))
    }

    /// The fourteen casts (§5.3).
    // An integer that a float cannot hold exactly is rounded to nearest,
    // which is what `as` does and what the instructions are defined to do.
    #[allow(clippy::cast_precision_loss)]
    fn cast(&self, op: Opcode, ty: TypeId, arg: Val) -> Result<Val, Stop> {
        let to = self.class(ty)?;
        Ok(match (op, arg, to) {
            (Opcode::Trunc | Opcode::ZExt, Val::Int { value, .. }, Class::Int(bits)) => {
                Val::int(bits, value)
            }
            (Opcode::SExt, Val::Int { bits: from, value }, Class::Int(bits)) => {
                Val::int(bits, sext(from, value) as u128)
            }
            (Opcode::FPTrunc, Val::Double(bits), Class::Float) => {
                self.float(f64::from_bits(bits) as f32)?
            }
            (Opcode::FPExt, Val::Float(bits), Class::Double) => {
                self.double(f64::from(f32::from_bits(bits)))?
            }
            (Opcode::FPToUI | Opcode::FPToSI, Val::Float(from), Class::Int(bits)) => {
                let x = f64::from(f32::from_bits(from));
                Val::Int { bits, value: ops::float_to_int(x, bits, op == Opcode::FPToSI)? }
            }
            (Opcode::FPToUI | Opcode::FPToSI, Val::Double(from), Class::Int(bits)) => {
                let x = f64::from_bits(from);
                Val::Int { bits, value: ops::float_to_int(x, bits, op == Opcode::FPToSI)? }
            }
            (Opcode::UIToFP, Val::Int { value, .. }, Class::Float) => Val::f32(value as f32),
            (Opcode::UIToFP, Val::Int { value, .. }, Class::Double) => Val::f64(value as f64),
            (Opcode::SIToFP, Val::Int { bits, value }, Class::Float) => {
                Val::f32(sext(bits, value) as f32)
            }
            (Opcode::SIToFP, Val::Int { bits, value }, Class::Double) => {
                Val::f64(sext(bits, value) as f64)
            }
            (Opcode::PtrToInt | Opcode::PtrToAddr, Val::Ptr(addr), Class::Int(bits)) => {
                Val::int(bits, u128::from(addr))
            }
            (Opcode::IntToPtr, Val::Int { value, .. }, Class::Ptr) => Val::Ptr(value as u64),
            (Opcode::BitCast, Val::Int { bits: 32, value }, Class::Float) => {
                Val::Float(value as u32)
            }
            (Opcode::BitCast, Val::Int { bits: 64, value }, Class::Double) => {
                Val::Double(value as u64)
            }
            (Opcode::BitCast, Val::Float(bits), Class::Int(32)) => Val::i32(bits),
            (Opcode::BitCast, Val::Double(bits), Class::Int(64)) => Val::i64(bits),
            (Opcode::BitCast, val, _) if self.fits(&val, ty)? => val,
            (Opcode::AddrSpaceCast, ..) => return unsupported("inst:addrspacecast"),
            (_, arg, _) => return invalid(format!("{} of {arg} to {ty:?}", op.name())),
        })
    }

    /// The new value of an `atomicrmw` (§5.4).
    fn rmw(&self, op: RmwOp, old: &Val, operand: Val) -> Result<Val, Stop> {
        if op == RmwOp::Xchg {
            return Ok(operand);
        }
        Ok(match (old, operand) {
            (&Val::Int { bits, value: a }, Val::Int { value: b, .. }) => {
                let (sa, sb) = (sext(bits, a), sext(bits, b));
                let value = match op {
                    RmwOp::Add => a.wrapping_add(b),
                    RmwOp::Sub => a.wrapping_sub(b),
                    RmwOp::And => a & b,
                    RmwOp::Nand => !(a & b),
                    RmwOp::Or => a | b,
                    RmwOp::Xor => a ^ b,
                    RmwOp::Max => {
                        if sa >= sb {
                            a
                        } else {
                            b
                        }
                    }
                    RmwOp::Min => {
                        if sa <= sb {
                            a
                        } else {
                            b
                        }
                    }
                    RmwOp::UMax => a.max(b),
                    RmwOp::UMin => a.min(b),
                    RmwOp::UIncWrap => {
                        if a >= b {
                            0
                        } else {
                            a + 1
                        }
                    }
                    RmwOp::UDecWrap => {
                        if a == 0 || a > b {
                            b
                        } else {
                            a - 1
                        }
                    }
                    RmwOp::USubCond => {
                        if a >= b {
                            a - b
                        } else {
                            a
                        }
                    }
                    RmwOp::USubSat => a.saturating_sub(b),
                    _ => return invalid(format!("atomicrmw {} of an integer", op.name())),
                };
                Val::int(bits, value)
            }
            (&Val::Float(a), Val::Float(b)) if matches!(op, RmwOp::FAdd | RmwOp::FSub) => {
                let op = if op == RmwOp::FAdd { Opcode::FAdd } else { Opcode::FSub };
                self.float(ops::f32_binary(op, f32::from_bits(a), f32::from_bits(b)))?
            }
            (&Val::Double(a), Val::Double(b)) if matches!(op, RmwOp::FAdd | RmwOp::FSub) => {
                let op = if op == RmwOp::FAdd { Opcode::FAdd } else { Opcode::FSub };
                self.double(ops::f64_binary(op, f64::from_bits(a), f64::from_bits(b)))?
            }
            (Val::Float(_) | Val::Double(_), _) if op.is_float() => {
                return unsupported(format!("inst:atomicrmw-{}", op.name()));
            }
            _ => return invalid(format!("atomicrmw {} of these operands", op.name())),
        })
    }

    /// The `{ T, i1 }` of `cmpxchg` and of the `*.with.overflow` family.
    fn pair(&self, ty: TypeId, first: &Val, flag: &Val, flag_ty: TypeId) -> Result<Val, Stop> {
        let types = &self.module.types;
        let mut image = vec![0u8; self.image_len(ty)?];
        let at = match types.field_offsets(ty) {
            Some(&[0, at]) if flag_ty == TypeId::I1 => at as usize,
            _ => return invalid("the result type is not a value and an i1"),
        };
        let len = match first {
            Val::Int { bits, .. } => Class::Int(*bits).store_len(),
            Val::Ptr(_) => 8,
            _ => return invalid("the first field is not an integer or a pointer"),
        };
        if len > at || at >= image.len() {
            return invalid("the result type does not hold its fields");
        }
        encode(first, &mut image[..len]);
        encode(flag, &mut image[at..=at]);
        Ok(Val::Agg(Agg { ty, bytes: image.into() }))
    }

    // ---- calls ----

    fn call(&mut self, f: &Frame<'a>, call: CallSite<'a>, ty: TypeId) -> Result<Flow, Stop> {
        if call.bundle_count() != 0 {
            return unsupported("call:operand-bundle");
        }
        let callee = match call.callee() {
            Callee::Intrinsic(intrinsic) => {
                return Ok(match self.intrinsic(f, intrinsic, call, ty)? {
                    Some(val) => Flow::Value(val),
                    None => Flow::Next,
                });
            }
            Callee::Value(value) => value,
        };
        let module = self.module;
        let direct = callee
            .as_sym()
            .filter(|sym| sym.index() < module.symbol_count() && module.symbol(*sym).is_function());
        let target = if let Some(sym) = direct {
            // A direct call of a function without an address here.
            self.sym_addr(sym)?;
            sym
        } else {
            let addr = self.ptr(f, callee)?;
            self.mem.function_at(addr).ok_or(Trap::NotAFunction)?
        };
        let symbol = module.symbol(target);
        if symbol.ty() != call.fn_type() {
            return Err(Trap::FunctionTypeMismatch.into());
        }
        let SymbolKind::Function { cc, .. } = symbol.kind() else {
            return invalid(format!("{target:?} is not a function"));
        };
        if cc != call.cc() {
            return Err(Trap::CallConvMismatch.into());
        }
        let ret = match module.types.kind(call.fn_type()) {
            TypeKind::Function { variadic: true, .. } => return unsupported("call:variadic"),
            TypeKind::Function { ret, .. } => ret,
            _ => return invalid("a call with a type that is not a function type"),
        };
        let mut args = Vec::with_capacity(call.args().len());
        for (index, &arg) in call.args().iter().enumerate() {
            for attrs in [call.arg_attrs(index), symbol.param_attrs(index)] {
                match attrs.mem() {
                    ParamMem::ByVal => return unsupported("attr:byval"),
                    ParamMem::InAlloca => return unsupported("attr:inalloca"),
                    ParamMem::Preallocated => return unsupported("attr:preallocated"),
                    _ => {}
                }
            }
            args.push(self.get(f, arg)?);
        }
        if let Some(body) = self.program.body(target) {
            self.push_frame(target, body, args, call.is_noreturn())?;
            return Ok(Flow::Called);
        }
        if self.host_of[target.index()] == 0 && self.builtin_of[target.index()] != Builtin::None {
            return self.builtin(target, &args, ret, call.is_noreturn());
        }
        let Some(index) = self.host_of[target.index()].checked_sub(1) else {
            return unsupported(format!("extern:{}", String::from_utf8_lossy(symbol.name())));
        };
        let index = index as usize;
        let mut cx = HostCx { memory: &mut self.mem, index: self.host_calls[index] };
        let result = (self.hosts[index].call)(&mut cx, &args)?;
        let fitting = match (&result, self.class(ret)?) {
            (None, Class::Void) => true,
            (Some(val), _) => self.fits(val, ret)?,
            (None, _) => false,
        };
        if !fitting {
            let name = &self.hosts[index].name;
            return invalid(format!("host function {name} returned {result:?} as a {ret:?}"));
        }
        self.host_calls[index] += 1;
        self.trace.push(Event {
            name: Arc::clone(&self.hosts[index].name),
            args,
            result: result.clone(),
        });
        if call.is_noreturn() {
            return Err(Trap::NoReturnReturned.into());
        }
        let _ = ty;
        Ok(result.map_or(Flow::Next, Flow::Value))
    }

    /// A call of a function of the exception model (§5.9).
    fn builtin(
        &mut self,
        target: SymId,
        args: &[Val],
        ret: TypeId,
        noreturn: bool,
    ) -> Result<Flow, Stop> {
        match (self.builtin_of[target.index()], args, ret) {
            (Builtin::Throw, [Val::Ptr(ty), payload @ Val::Int { bits: 64, .. }], TypeId::VOID) => {
                if *ty == 0 {
                    // The model's own refusal, as a host function's.
                    return Err(Trap::Host.into());
                }
                self.throw(*ty, payload.clone())?;
                Ok(Flow::Unwound)
            }
            (Builtin::Catch, [Val::Ptr(addr)], TypeId::I64) => {
                let exn = self.exns.iter_mut().find(|exn| exn.addr == *addr);
                match exn {
                    Some(exn) if exn.state == ExnState::Landed && exn.landed_handler => {
                        exn.state = ExnState::Done;
                        if noreturn {
                            return Err(Trap::NoReturnReturned.into());
                        }
                        Ok(Flow::Value(exn.payload.clone()))
                    }
                    _ => Err(Trap::NotAnException.into()),
                }
            }
            _ => {
                let name = String::from_utf8_lossy(self.module.symbol(target).name());
                invalid(format!("{name} is declared with another type"))
            }
        }
    }

    // ---- unwinding (§5.9) ----

    /// Throws an exception of type `ty` from the call the innermost frame
    /// is at: the search phase, then the cleanup phase.
    fn throw(&mut self, ty: u64, payload: Val) -> Result<(), Stop> {
        let addr = self.mem.reserve()?;
        // Phase 1: the first frame, inside out, whose landing pad has a
        // handler. A pad that only cleans up does not stop the search.
        let mut left: Option<SymId> = None;
        let mut handler = None;
        for index in (0..self.frames.len()).rev() {
            let f = self.frames[index];
            let call = self.unwound_call(&f, left)?;
            if let Some(pad) = call.and_then(|c| c.unwind_dest())
                && let PadMatch::Handler(_) = self.pad_match(&f, pad, ty)?
            {
                handler = Some((f.serial, f.pc));
                break;
            }
            left = Some(f.sym);
            self.tick(1)?;
        }
        let Some((serial, pc)) = handler else {
            // `__cxa_throw` calls `std::terminate`; no cleanup runs.
            return Err(Trap::UncaughtException.into());
        };
        self.exns.push(Exn {
            addr,
            ty,
            payload,
            handler: serial,
            handler_pc: pc,
            state: ExnState::Raised,
            landed_handler: false,
        });
        self.cleanup_phase(self.exns.len() - 1, None)
    }

    /// The call through which an exception leaves the frame `f`, after
    /// leaving the function `left` (none: the thrower itself), with the
    /// `nounwind` checks of crossing them.
    fn unwound_call(
        &self,
        f: &Frame<'a>,
        left: Option<SymId>,
    ) -> Result<Option<CallSite<'a>>, Stop> {
        let Some(call) = f.body.inst(InstId::new(f.pc)).call() else {
            return invalid("an exception left something that is not a call");
        };
        // The function left: the callee of a body, else the thrower.
        let leaving = left.or_else(|| call.callee_value().as_sym());
        if call.is_nounwind() || leaving.is_some_and(|sym| self.is_nounwind_fn(sym)) {
            return Err(Trap::UnwindFromNounwind.into());
        }
        Ok(Some(call))
    }

    /// `nounwind` on the function `sym`.
    fn is_nounwind_fn(&self, sym: SymId) -> bool {
        sym.index() < self.module.symbol_count()
            && matches!(self.module.symbol(sym).kind(), SymbolKind::Function { attrs, .. }
                if self.module.fn_attrs(attrs).has(FnAttrs::NOUNWIND))
    }

    /// Phase 2 for exception `index`: frames are left until one has a
    /// landing pad that cleans up or is the handler the search found.
    /// `left` is the function the exception just left (`resume`).
    fn cleanup_phase(&mut self, index: usize, mut left: Option<SymId>) -> Result<(), Stop> {
        let (ty, handler, handler_pc) = {
            let exn = &self.exns[index];
            (exn.ty, exn.handler, exn.handler_pc)
        };
        loop {
            let Some(&f) = self.frames.last() else {
                return Err(Trap::EhPhaseMismatch.into());
            };
            let call = self.unwound_call(&f, left)?;
            let at_handler = f.serial == handler;
            if at_handler && f.pc != handler_pc {
                return Err(Trap::EhPhaseMismatch.into());
            }
            if let Some(pad) = call.and_then(|c| c.unwind_dest()) {
                let selector = match (self.pad_match(&f, pad, ty)?, at_handler) {
                    (PadMatch::Handler(selector), true) => Some(selector),
                    // libstdc++ would install it, libc++abi terminates.
                    (PadMatch::Cleanup, false) => Some(0),
                    (PadMatch::Nothing, false) => None,
                    // A handler where the search found none: libstdc++
                    // would install it, libc++abi terminates. And no
                    // handler where the search found one.
                    (PadMatch::Handler(_), false) | (_, true) => {
                        return Err(Trap::EhPhaseMismatch.into());
                    }
                };
                if let Some(selector) = selector {
                    let exn = &mut self.exns[index];
                    exn.state = ExnState::Landed;
                    exn.landed_handler = at_handler;
                    self.landing = Some((exn.addr, selector));
                    return self.jump(&f, pad);
                }
            } else if at_handler {
                return Err(Trap::EhPhaseMismatch.into());
            }
            left = Some(f.sym);
            self.tick(1)?;
            self.pop_frame();
        }
    }

    /// `resume` of the exception at `addr` in the innermost frame.
    fn resume(&mut self, addr: u64) -> Result<(), Stop> {
        let Some(index) = self.exns.iter().position(|exn| exn.addr == addr) else {
            return Err(Trap::NotAnException.into());
        };
        if self.exns[index].state != ExnState::Landed {
            return Err(Trap::NotAnException.into());
        }
        let Some(&f) = self.frames.last() else {
            return invalid("no frame");
        };
        let handler = self.exns[index].handler;
        // The handler must be a frame outside this one, still running.
        if handler >= f.serial || !self.frames.iter().any(|frame| frame.serial == handler) {
            return Err(Trap::EhPhaseMismatch.into());
        }
        self.exns[index].state = ExnState::Raised;
        self.pop_frame();
        self.cleanup_phase(index, Some(f.sym))
    }

    /// What the personality does at landing pad `pad` of frame `f` with an
    /// exception of type `ty`: the clauses in order, the first match wins.
    fn pad_match(&mut self, f: &Frame<'a>, pad: BlockId, ty: u64) -> Result<PadMatch, Stop> {
        let table = self.eh_table(f)?;
        let Some(inst) = f.body.block(pad).insts().find(|i| i.opcode() == Opcode::LandingPad)
        else {
            return invalid(format!("{pad:?} is not a landing pad"));
        };
        let Some(clauses) = table.pads.get(&inst.id()) else {
            return invalid(format!("{pad:?} is not a landing pad"));
        };
        for clause in clauses {
            match clause {
                EhClause::Catch { ty: caught, id } if *caught == 0 || *caught == ty => {
                    return Ok(PadMatch::Handler(*id));
                }
                EhClause::Filter { tys, id } if !tys.contains(&ty) => {
                    return Ok(PadMatch::Handler(*id));
                }
                _ => {}
            }
        }
        let InstKind::LandingPad(lp) = inst.kind() else {
            return invalid(format!("{pad:?} is not a landing pad"));
        };
        Ok(if lp.is_cleanup() { PadMatch::Cleanup } else { PadMatch::Nothing })
    }

    /// The type-id table of frame `f`'s function, built on first use.
    fn eh_table(&mut self, f: &Frame<'a>) -> Result<Rc<EhTable>, Stop> {
        if let Some(table) = self.eh_tables.get(&f.sym) {
            return Ok(Rc::clone(table));
        }
        let SymbolKind::Function { personality, .. } = self.module.symbol(f.sym).kind() else {
            return invalid(format!("{:?} is not a function", f.sym));
        };
        let known = personality
            .is_some_and(|sym| self.module.symbol(sym).name() == eh::PERSONALITY.as_bytes());
        if !known {
            return unsupported("eh:personality");
        }
        let mut table = EhTable::default();
        let id_of = |types: &mut Vec<u64>, ty: u64| -> i32 {
            let index = types.iter().position(|&t| t == ty).unwrap_or_else(|| {
                types.push(ty);
                types.len() - 1
            });
            index as i32 + 1
        };
        let mut filters = 0;
        for pad in f.body.landing_pads() {
            let mut clauses = Vec::new();
            for &clause in pad.clauses() {
                match clause {
                    Clause::Catch(c) => {
                        let ty = self.type_info(c)?;
                        clauses.push(EhClause::Catch { ty, id: id_of(&mut table.types, ty) });
                    }
                    Clause::Filter(c) => {
                        let tys = self.filter_types(c)?;
                        for &ty in &tys {
                            id_of(&mut table.types, ty);
                        }
                        filters += 1;
                        clauses.push(EhClause::Filter { tys, id: -filters });
                    }
                }
            }
            table.pads.insert(pad.inst(), clauses);
        }
        for call in f.body.calls() {
            if call.callee() == Callee::Intrinsic(Intrinsic::EhTypeidFor)
                && let Some(&arg) = call.args().first()
            {
                let ty = self.static_ptr(arg)?;
                id_of(&mut table.types, ty);
            }
        }
        let table = Rc::new(table);
        self.eh_tables.insert(f.sym, Rc::clone(&table));
        Ok(table)
    }

    /// A pointer operand that is a constant or a symbol.
    fn static_ptr(&self, value: Value) -> Result<u64, Stop> {
        let val = match value.kind() {
            ValueKind::Const(id) => self.constant(id)?,
            ValueKind::Sym(sym) => Val::Ptr(self.sym_addr(sym)?),
            _ => return invalid("a type-information operand that is not a constant"),
        };
        match val {
            Val::Ptr(addr) => Ok(addr),
            other => invalid(format!("{other} where a type-information pointer is required")),
        }
    }

    /// A `catch` clause's type-information address; 0 is catch-all.
    fn type_info(&self, c: ConstId) -> Result<u64, Stop> {
        match self.constant(c)? {
            Val::Ptr(addr) => Ok(addr),
            other => invalid(format!("{other} where a type-information pointer is required")),
        }
    }

    /// The types a `filter` clause lists.
    fn filter_types(&self, c: ConstId) -> Result<Vec<u64>, Stop> {
        let elems: &[ConstId] = match self.module.consts.get(c) {
            Const::Aggregate { elems, .. } => elems,
            Const::Zero(ty) | Const::Undef(ty) | Const::Poison(ty)
                if matches!(self.module.types.kind(ty), TypeKind::Array { len: 0, .. }) =>
            {
                &[]
            }
            _ => return unsupported("eh:filter-form"),
        };
        let mut tys = Vec::with_capacity(elems.len());
        for &elem in elems {
            match self.type_info(elem)? {
                0 => return unsupported("eh:filter-catch-all"),
                ty => tys.push(ty),
            }
        }
        Ok(tys)
    }

    /// A two-field struct of type `ty` holding `first` and `second`.
    fn pair_of(&self, ty: TypeId, first: &Val, second: &Val) -> Result<Val, Stop> {
        let mut image = vec![0u8; self.image_len(ty)?];
        let Some(&[0, at]) = self.module.types.field_offsets(ty) else {
            return invalid("the type is not a pair");
        };
        let at = at as usize;
        if at < 8 || at + 4 > image.len() {
            return invalid("the type does not hold its fields");
        }
        encode(first, &mut image[..8]);
        encode(second, &mut image[at..at + 4]);
        Ok(Val::Agg(Agg { ty, bytes: image.into() }))
    }

    /// An intrinsic call (§5.6). `None` for an intrinsic without a result.
    #[allow(clippy::too_many_lines)]
    fn intrinsic(
        &mut self,
        f: &Frame<'a>,
        intrinsic: Intrinsic<'a>,
        call: CallSite<'a>,
        ty: TypeId,
    ) -> Result<Option<Val>, Stop> {
        let args = call.args();
        let arg = |index: usize| match args.get(index) {
            Some(&value) => Ok(value),
            None => invalid(format!("{} needs argument {index}", intrinsic.name())),
        };
        let int_op = |op: IntOp| -> Result<Option<Val>, Stop> {
            let (bits, a) = self.int(f, arg(0)?)?;
            let b = if args.len() > 1 { self.int(f, arg(1)?)?.1 } else { 0 };
            let c = if args.len() > 2 { self.int(f, arg(2)?)?.1 } else { 0 };
            Ok(Some(Val::Int { bits, value: ops::int_intrinsic(op, bits, a, b, c)? }))
        };
        let checked = |op: Checked| -> Result<Option<Val>, Stop> {
            let (bits, a) = self.int(f, arg(0)?)?;
            let (_, b) = self.int(f, arg(1)?)?;
            let (value, overflow) = ops::checked(op, bits, a, b);
            let Some(&[_, flag_ty]) = self.module.types.struct_fields(ty) else {
                return invalid("the result of a checked operation is not a pair");
            };
            Ok(Some(self.pair(ty, &Val::Int { bits, value }, &Val::i1(overflow), flag_ty)?))
        };
        let align = |index: usize| call.arg_attrs(index).align().unwrap_or(1);
        match intrinsic {
            Intrinsic::Memcpy
            | Intrinsic::MemcpyInline
            | Intrinsic::Memmove
            | Intrinsic::Memset
            | Intrinsic::MemsetInline => {
                let dst = self.ptr(f, arg(0)?)?;
                let (_, len) = self.int(f, arg(2)?)?;
                let len = u64::try_from(len).map_err(|_| Trap::OutOfBounds)?;
                if matches!(intrinsic, Intrinsic::Memset | Intrinsic::MemsetInline) {
                    let (_, byte) = self.int(f, arg(1)?)?;
                    self.mem.fill((dst, align(0)), byte as u8, len)?;
                    return Ok(None);
                }
                let src = self.ptr(f, arg(1)?)?;
                let apart = dst.abs_diff(src) >= len;
                if intrinsic != Intrinsic::Memmove && !apart && dst != src && len != 0 {
                    return Err(Trap::OverlappingCopy.into());
                }
                self.mem.copy((dst, align(0)), (src, align(1)), len)?;
                Ok(None)
            }
            Intrinsic::Abs => int_op(IntOp::Abs),
            Intrinsic::Smax => int_op(IntOp::Smax),
            Intrinsic::Smin => int_op(IntOp::Smin),
            Intrinsic::Umax => int_op(IntOp::Umax),
            Intrinsic::Umin => int_op(IntOp::Umin),
            Intrinsic::Ctpop => int_op(IntOp::Ctpop),
            Intrinsic::Ctlz => int_op(IntOp::Ctlz),
            Intrinsic::Cttz => int_op(IntOp::Cttz),
            Intrinsic::BitReverse => int_op(IntOp::BitReverse),
            Intrinsic::Fshl => int_op(IntOp::Fshl),
            Intrinsic::Fshr => int_op(IntOp::Fshr),
            Intrinsic::SaddSat => int_op(IntOp::SaddSat),
            Intrinsic::UaddSat => int_op(IntOp::UaddSat),
            Intrinsic::SsubSat => int_op(IntOp::SsubSat),
            Intrinsic::UsubSat => int_op(IntOp::UsubSat),
            Intrinsic::Bswap => match self.int(f, arg(0)?)? {
                (bits, _) if bits % 16 != 0 => invalid("bswap of an odd number of bytes"),
                _ => int_op(IntOp::Bswap),
            },
            Intrinsic::SaddWithOverflow => checked(Checked::Sadd),
            Intrinsic::UaddWithOverflow => checked(Checked::Uadd),
            Intrinsic::SsubWithOverflow => checked(Checked::Ssub),
            Intrinsic::UsubWithOverflow => checked(Checked::Usub),
            Intrinsic::SmulWithOverflow => checked(Checked::Smul),
            Intrinsic::UmulWithOverflow => checked(Checked::Umul),
            Intrinsic::Scmp | Intrinsic::Ucmp => {
                let (bits, a) = self.int(f, arg(0)?)?;
                let (_, b) = self.int(f, arg(1)?)?;
                let Class::Int(result_bits) = self.class(ty)? else {
                    return invalid("a three-way compare with a result that is not an integer");
                };
                let signed = intrinsic == Intrinsic::Scmp;
                let value = ops::three_way(signed, bits, a, b, result_bits);
                Ok(Some(Val::Int { bits: result_bits, value }))
            }
            Intrinsic::Fabs | Intrinsic::CopySign => {
                // Bit operations: exact whatever the operands are.
                let sign = if intrinsic == Intrinsic::Fabs { None } else { Some(arg(1)?) };
                Ok(Some(match (self.get(f, arg(0)?)?, sign) {
                    (Val::Float(bits), None) => Val::Float(bits & !(1 << 31)),
                    (Val::Double(bits), None) => Val::Double(bits & !(1 << 63)),
                    (Val::Float(bits), Some(sign)) => match self.get(f, sign)? {
                        Val::Float(sign) => Val::Float((bits & !(1 << 31)) | (sign & (1 << 31))),
                        _ => return invalid("copysign of two types"),
                    },
                    (Val::Double(bits), Some(sign)) => match self.get(f, sign)? {
                        Val::Double(sign) => Val::Double((bits & !(1 << 63)) | (sign & (1 << 63))),
                        _ => return invalid("copysign of two types"),
                    },
                    _ => {
                        self.class(ty)?;
                        return invalid("a float intrinsic on something else");
                    }
                }))
            }
            Intrinsic::Sqrt
            | Intrinsic::Floor
            | Intrinsic::Ceil
            | Intrinsic::Trunc
            | Intrinsic::Round
            | Intrinsic::RoundEven
            | Intrinsic::Rint
            | Intrinsic::NearbyInt => {
                macro_rules! rounded {
                    ($x:expr) => {
                        match intrinsic {
                            Intrinsic::Sqrt => $x.sqrt(),
                            Intrinsic::Floor => $x.floor(),
                            Intrinsic::Ceil => $x.ceil(),
                            Intrinsic::Trunc => $x.trunc(),
                            Intrinsic::Round => $x.round(),
                            // The default rounding mode is the only one.
                            _ => $x.round_ties_even(),
                        }
                    };
                }
                Ok(Some(match self.get(f, arg(0)?)? {
                    Val::Float(bits) => self.float(rounded!(f32::from_bits(bits)))?,
                    Val::Double(bits) => self.double(rounded!(f64::from_bits(bits)))?,
                    _ => {
                        self.class(ty)?;
                        return invalid("a float intrinsic on something else");
                    }
                }))
            }
            Intrinsic::Fma => {
                let (a, b, c) = (arg(0)?, arg(1)?, arg(2)?);
                Ok(Some(match (self.get(f, a)?, self.get(f, b)?, self.get(f, c)?) {
                    (Val::Float(a), Val::Float(b), Val::Float(c)) => {
                        self.float(f32::from_bits(a).mul_add(f32::from_bits(b), f32::from_bits(c)))?
                    }
                    (Val::Double(a), Val::Double(b), Val::Double(c)) => self
                        .double(f64::from_bits(a).mul_add(f64::from_bits(b), f64::from_bits(c)))?,
                    _ => {
                        self.class(ty)?;
                        return invalid("fma on something that is not three floats");
                    }
                }))
            }
            Intrinsic::Trap => Err(Trap::TrapIntrinsic.into()),
            Intrinsic::EhTypeidFor => {
                let ty = self.ptr(f, arg(0)?)?;
                let table = self.eh_table(f)?;
                match table.types.iter().position(|&t| t == ty) {
                    Some(index) => Ok(Some(Val::i32(index as u32 + 1))),
                    None => invalid("llvm.eh.typeid.for of a type outside the table"),
                }
            }
            Intrinsic::Other(name) => match no_code_intrinsic(name) {
                // `spec/ir.md` §5.5: these generate no code. A result that
                // is dropped stands for poison, which is zero.
                Some(NoCode::Drop | NoCode::DebugRecord) => match self.class(ty)? {
                    Class::Void => Ok(None),
                    _ => Ok(Some(self.zero(ty)?)),
                },
                Some(NoCode::FirstArg) => Ok(Some(self.get(f, arg(0)?)?)),
                None => unsupported(format!("intrinsic:{name}")),
            },
            other => unsupported(format!("intrinsic:{}", other.name())),
        }
    }
}
