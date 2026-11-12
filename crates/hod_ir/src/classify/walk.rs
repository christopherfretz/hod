//! The walk (`spec/ir.md` §8.6 to §8.8): one pass over a module's tables
//! or over a body, consulting the support matrix and the structural
//! rules, in the order §8.8 fixes.
//!
//! The walk reports to a [`Sink`]. `classify` gives it one that stops at
//! the first finding; the census gives it one that counts and goes on.
//! Nothing here allocates or formats: a finding is a row index or a
//! borrowed name until a sink turns it into a [`Rejection`].

use std::borrow::Cow;
use std::ops::ControlFlow;

use super::attrs::{call_declaration, merge_call_attrs};
use super::matrix::{FAR_PTR, NO_VALUE, SUPPORT_ROWS, TypeClass, at, class_code, rules};
use super::support::Support;
use super::{Rejection, RejectionKind, StackArgs};
use crate::attrs::{CallConv, FnAttrs, FnAttrsView, ParamAttrs, ParamMem, UwTable};
use crate::body::{Body, CallSite, Clause};
use crate::constant::Const;
use crate::ids::{BlockId, ConstId, SymId, TypeId};
use crate::inst::{InstData, MemFlags, Opcode, RmwOp, TailKind};
use crate::intrinsic::Intrinsic;
use crate::module::Module;
use crate::props::{FramePointer, PicLevel};
use crate::symbol::{Symbol, SymbolKind, TlsModel, UnnamedAddr, Visibility};
use crate::types::TypeKind;
use crate::value::{InstId, Value, ValueKind};
use crate::{DiNode, EmissionKind};

pub(crate) type Flow = ControlFlow<()>;

/// The return and parameter attributes of the declaration a call takes
/// attributes from ([`call_declaration`]), if it has any.
fn declared_attrs<'m>(
    module: &'m Module,
    site: &CallSite<'_>,
) -> Option<(ParamAttrs, &'m [ParamAttrs])> {
    match call_declaration(module, site)?.kind() {
        SymbolKind::Function { ret_attrs, param_attrs, .. }
            if !ret_attrs.is_none() || !param_attrs.is_empty() =>
        {
            Some((ret_attrs, param_attrs))
        }
        _ => None,
    }
}

const GO: Flow = ControlFlow::Continue(());

/// How deep a constant may nest before the walk gives up on it
/// (`limit:constant-nesting`): the walk recurses once per level.
pub(crate) const MAX_CONSTANT_NESTING: u32 = 64;

/// Where a finding was made.
#[derive(Clone, Copy, Debug)]
pub(crate) struct At {
    pub(crate) symbol: Option<SymId>,
    pub(crate) inst: Option<InstId>,
}

impl At {
    const MODULE: At = At { symbol: None, inst: None };

    const fn symbol(symbol: SymId) -> At {
        At { symbol: Some(symbol), inst: None }
    }
}

/// One thing the walk found, before it has a name.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Finding<'a> {
    /// A matrix row that is not supported.
    Row(usize),
    /// A supported row met with a class it does not have.
    Pair(usize, TypeClass),
    /// A structural rule.
    Rule(&'static str),
    /// A malformed-input outcome.
    Invalid(&'static str),
    /// `prefix` then a name of the input: an unknown intrinsic or bundle.
    Named(&'static str, &'a str),
    /// A module flag hod neither knows nor may ignore.
    Flag(&'a [u8]),
    /// A calling convention without a keyword.
    Cc(u16),
    /// A producer record.
    Record(&'a str),
}

impl Finding<'_> {
    /// The finding's kind and its construct name. Allocates for the
    /// names that are built from the input.
    pub(crate) fn name(self) -> (RejectionKind, Cow<'static, str>) {
        let name = match self {
            Finding::Row(row) => Cow::Borrowed(SUPPORT_ROWS[row].name),
            Finding::Pair(row, class) => {
                Cow::Owned(format!("{}/{}", SUPPORT_ROWS[row].name, class.name()))
            }
            Finding::Rule(name) => Cow::Borrowed(name),
            Finding::Invalid(name) => return (RejectionKind::Invalid, Cow::Borrowed(name)),
            Finding::Named(prefix, name) => Cow::Owned(format!("{prefix}{name}")),
            Finding::Flag(name) => {
                Cow::Owned(format!("module-flag:{}", String::from_utf8_lossy(name)))
            }
            Finding::Cc(number) => Cow::Owned(format!("cc:{number}")),
            Finding::Record(name) => Cow::Owned(name.to_owned()),
        };
        (RejectionKind::Unsupported, name)
    }
}

/// Where findings go. `Break` ends the walk.
pub(crate) trait Sink {
    fn report(&mut self, finding: Finding<'_>, at: At) -> Flow;
}

/// The sink of `classify`: keeps the first finding and stops.
#[derive(Debug, Default)]
pub(crate) struct First(pub(crate) Option<Rejection>);

impl Sink for First {
    #[cold]
    fn report(&mut self, finding: Finding<'_>, at: At) -> Flow {
        let (kind, construct) = finding.name();
        self.0 = Some(Rejection { kind, construct, symbol: at.symbol, inst: at.inst });
        ControlFlow::Break(())
    }
}

/// A sink that only notes that there was a finding.
#[derive(Debug, Default)]
struct Probe(bool);

impl Sink for Probe {
    fn report(&mut self, _: Finding<'_>, _: At) -> Flow {
        self.0 = true;
        ControlFlow::Break(())
    }
}

/// The row of the C calling convention.
const CCC: usize = match at::cc(0) {
    Some(row) => row,
    None => panic!("`ccc` has a row"),
};

/// Whether the class set `classes` has the class with code `code`
/// ([`class_code`]). The two codes that are not classes select bits no
/// set has.
#[inline]
const fn has_class(classes: u16, code: u8) -> bool {
    classes as u32 & (1 << (code & 31)) != 0
}

/// Where a constant is being read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Place {
    /// A variable's initializer.
    Init,
    /// An instruction operand.
    Operand,
    /// A landing-pad clause.
    Clause,
}

/// The body being walked and what the walk reads of its function.
struct Cx<'a> {
    body: &'a Body,
    sym: SymId,
    symbol: Symbol<'a>,
    params: &'a [TypeId],
}

impl Cx<'_> {
    /// The type of `value` as far as it can be told without a check;
    /// `void` otherwise.
    fn type_of(&self, module: &Module, value: Value) -> TypeId {
        match value.kind() {
            ValueKind::Inst(id) => {
                self.body.insts.get(id.index()).map_or(TypeId::VOID, |inst| inst.ty)
            }
            ValueKind::Arg(index) => {
                self.params.get(index as usize).copied().unwrap_or(TypeId::VOID)
            }
            ValueKind::Const(id) if id.index() < module.consts.len() => module.consts.ty(id),
            ValueKind::Sym(_) | ValueKind::Slot(_) => TypeId::PTR,
            _ => TypeId::VOID,
        }
    }
}

const PARAM_FLAGS: [u16; 7] = [
    ParamAttrs::ZEROEXT,
    ParamAttrs::SIGNEXT,
    ParamAttrs::INREG,
    ParamAttrs::NEST,
    ParamAttrs::SWIFT_SELF,
    ParamAttrs::SWIFT_ERROR,
    ParamAttrs::SWIFT_ASYNC,
];

/// The row of the bundle tag `tag`.
fn bundle_row(tag: &str) -> Option<usize> {
    (at::BUNDLE..at::BUNDLE + at::BUNDLE_COUNT)
        .find(|&row| SUPPORT_ROWS[row].name.strip_prefix("bundle:") == Some(tag))
}

/// Whether `module` has a compile unit `llc` writes debug information
/// for: one that is not `NoDebug` (a module without one gets none, so
/// its `Dwarf Version` does not matter).
fn writes_debug_info(module: &Module) -> bool {
    let debug = &module.debug;
    debug.compile_units().iter().any(|&unit| {
        !matches!(debug.node(unit), DiNode::CompileUnit(cu) if cu.emission_kind == EmissionKind::NoDebug)
    })
}

pub(crate) struct Walker<'a, S> {
    pub(crate) module: &'a Module,
    pub(crate) support: &'a Support,
    pub(crate) stack_args: Option<&'a dyn StackArgs>,
    pub(crate) sink: &'a mut S,
}

impl<'a, S: Sink> Walker<'a, S> {
    // ---- reporting ----

    /// Consults a row.
    #[inline]
    fn row(&mut self, row: usize, at: At) -> Flow {
        if self.support.mask[row] == 0 { self.sink.report(Finding::Row(row), at) } else { GO }
    }

    fn rule(&mut self, name: &'static str, at: At) -> Flow {
        self.sink.report(Finding::Rule(name), at)
    }

    fn invalid(&mut self, name: &'static str, at: At) -> Flow {
        self.sink.report(Finding::Invalid(name), at)
    }

    /// The classes row `row` can be given: its own, of the supported
    /// types.
    #[inline]
    fn classes(&self, row: usize) -> u16 {
        self.support.mask[row] & self.support.types
    }

    /// Checks one subject type of `row` (§8.3). `classes` is
    /// `self.classes(row)`.
    #[inline]
    fn subject(&mut self, row: usize, classes: u16, ty: TypeId, at: At) -> Flow {
        let code = class_code(&self.module.types, ty);
        if has_class(classes, code) {
            return GO;
        }
        self.subject_slow(row, code, rules::ADDRSPACE_POINTER, at)
    }

    #[cold]
    fn subject_slow(&mut self, row: usize, code: u8, far: &'static str, at: At) -> Flow {
        match code {
            NO_VALUE => self.invalid(rules::TYPE, at),
            FAR_PTR => self.rule(far, at),
            code => {
                let class = TypeClass::ALL[usize::from(code)];
                if self.support.mask[at::TYPE + usize::from(code)] == 0 {
                    self.sink.report(Finding::Row(at::TYPE + usize::from(code)), at)
                } else if self.support.mask[row] != 0 && self.support.mask[row] & class.bit() == 0 {
                    self.sink.report(Finding::Pair(row, class), at)
                } else {
                    // The row itself is not supported and has been reported.
                    GO
                }
            }
        }
    }

    /// Checks that `sym` is a symbol; with `tls` a rule name, also that
    /// it is not thread-local.
    #[inline]
    fn symbol_use(&mut self, sym: SymId, tls: Option<&'static str>, at: At) -> Flow {
        if sym.index() >= self.module.symbol_count() {
            return self.invalid(rules::SYMBOL, at);
        }
        match tls {
            Some(rule) if self.module.symbol(sym).tls() != TlsModel::None => self.rule(rule, at),
            _ => GO,
        }
    }

    fn cc(&mut self, cc: CallConv, at: At) -> Flow {
        match at::cc(cc.llvm_id()) {
            Some(row) => self.row(row, at),
            None => self.sink.report(Finding::Cc(cc.llvm_id()), at),
        }
    }

    #[inline]
    fn param_attrs(&mut self, attrs: ParamAttrs, at: At) -> Flow {
        if attrs.is_none() { GO } else { self.param_attrs_slow(attrs, false, at) }
    }

    /// `asm_operand`: the attributes of an inline-assembly operand, whose
    /// `elementtype` is the operand's type for the target to read
    /// (`spec/ir.md` §2.3), not a way of passing it.
    fn param_attrs_slow(&mut self, attrs: ParamAttrs, asm_operand: bool, at: At) -> Flow {
        for (index, &flag) in PARAM_FLAGS.iter().enumerate() {
            if attrs.has(flag) {
                self.row(at::ATTR_PARAM + index, at)?;
            }
        }
        if attrs.align().is_some() {
            self.row(at::ATTR_ALIGN, at)?;
        }
        if attrs.stack_align().is_some() {
            self.row(at::ATTR_ALIGNSTACK, at)?;
        }
        // Exhaustive on purpose: a new kind has to be given a row.
        let mem = match attrs.mem() {
            ParamMem::None => return GO,
            ParamMem::Sret => 0,
            ParamMem::ByVal => 1,
            ParamMem::ByRef => 2,
            ParamMem::InAlloca => 3,
            ParamMem::Preallocated => 4,
            ParamMem::ElementType if asm_operand => return GO,
            ParamMem::ElementType => 5,
        };
        self.row(at::ATTR_MEM + mem, at)
    }

    // ---- constants ----

    /// The row of an address constant: its own in an initializer and as
    /// an operand, the aggregate's inside an aggregate operand, none in
    /// a clause.
    fn address(&mut self, place: Place, nested: bool, init: usize, operand: usize, at: At) -> Flow {
        match place {
            Place::Init => self.row(init, at),
            Place::Operand if nested => self.row(at::OPERAND_ADDRESS_IN_AGGREGATE, at),
            Place::Operand => self.row(operand, at),
            Place::Clause => GO,
        }
    }

    /// Checks that `blockaddress(func, block)` names a block of a
    /// function with a body, and of the function walked when in a body.
    fn block_address(&mut self, cx: Option<&Cx<'_>>, func: SymId, block: BlockId, at: At) -> Flow {
        let module = self.module;
        if func.index() >= module.symbol_count() {
            return self.invalid(rules::SYMBOL, at);
        }
        match cx {
            Some(cx) if cx.sym == func => {
                if block.index() >= cx.body.blocks.len() {
                    self.invalid(rules::BLOCKADDRESS, at)?;
                }
            }
            Some(_) => return self.rule(rules::BLOCKADDRESS_OTHER, at),
            None => {
                let function = module.symbol(func).kind();
                if !matches!(function, SymbolKind::Function { has_body: true, .. }) {
                    self.invalid(rules::BLOCKADDRESS, at)?;
                }
            }
        }
        GO
    }

    /// Walks constant `id`, which exists. `nested` says it is an element
    /// of an aggregate; `depth` counts the constants around it.
    fn constant(
        &mut self,
        cx: Option<&Cx<'_>>,
        id: ConstId,
        place: Place,
        nested: bool,
        depth: u32,
        at: At,
    ) -> Flow {
        let module = self.module;
        if depth > MAX_CONSTANT_NESTING {
            return self.rule(rules::CONSTANT_NESTING, at);
        }
        match module.consts.get(id) {
            Const::Int { .. }
            | Const::WideInt { .. }
            | Const::Float { .. }
            | Const::WideFloat { .. }
            | Const::Null(_)
            | Const::Zero(_)
            | Const::Undef(_)
            | Const::Poison(_)
            | Const::Bytes { .. } => GO,
            Const::NoneToken => {
                if place == Place::Init {
                    self.invalid(rules::INITIALIZER, at)
                } else {
                    GO
                }
            }
            Const::Aggregate { elems, .. } => {
                for &elem in elems {
                    self.constant(cx, elem, place, true, depth + 1, at)?;
                }
                GO
            }
            Const::Splat { elem, .. } => self.constant(cx, elem, place, true, depth + 1, at),
            Const::Addr { sym, .. } => {
                // A thread-local address is code (WP-073): an operand, or
                // an offset from one, but never data.
                let code = place == Place::Operand && !nested && depth == 0;
                self.symbol_use(sym, (!code).then_some(rules::TLS_CONSTANT), at)?;
                self.address(place, nested, at::INIT_SYMBOL, at::OPERAND_SYMBOL, at)
            }
            Const::BlockAddr { func, block } => {
                self.block_address(cx, func, block, at)?;
                self.address(place, nested, at::INIT_BLOCKADDRESS, at::OPERAND_BLOCKADDRESS, at)
            }
            Const::SymDiff { a, b, plt, .. } => {
                self.symbol_use(a, Some(rules::TLS_CONSTANT), at)?;
                self.symbol_use(b, Some(rules::TLS_CONSTANT), at)?;
                let init = if plt { at::INIT_SYM_DIFF_PLT } else { at::INIT_SYM_DIFF };
                self.address(place, nested, init, at::OPERAND_SYM_DIFF, at)
            }
            Const::IntOfAddr { addr, .. } => {
                self.address(place, nested, at::INIT_INT_OF_ADDR, at::OPERAND_INT_OF_ADDR, at)?;
                self.constant(cx, addr, place, nested, depth + 1, at)
            }
            Const::AddrOfInt { .. } => {
                if place == Place::Operand && !nested {
                    self.row(at::OPERAND_ADDR_OF_INT, at)
                } else {
                    // A number: data like any other.
                    GO
                }
            }
            Const::DsoLocalEquivalent(sym) => {
                self.symbol_use(sym, Some(rules::TLS_CONSTANT), at)?;
                let (init, operand) =
                    (at::INIT_DSO_LOCAL_EQUIVALENT, at::OPERAND_DSO_LOCAL_EQUIVALENT);
                self.address(place, nested, init, operand, at)
            }
            Const::NoCfi(sym) => {
                self.symbol_use(sym, Some(rules::TLS_CONSTANT), at)?;
                self.address(place, nested, at::INIT_NO_CFI, at::OPERAND_NO_CFI, at)
            }
            Const::Expr { op, args, .. } => {
                if let Some(diff) = module.consts.block_diff(id) {
                    // A label difference (WP-095): its two blocks are
                    // checked as `blockaddress` constants are.
                    for block in [diff.a, diff.b] {
                        self.block_address(cx, diff.func, block, at)?;
                    }
                    // Inside an aggregate operand too: no lowering forms it.
                    return match place {
                        Place::Init => self.row(at::INIT_BLOCK_DIFF, at),
                        Place::Operand => self.row(at::OPERAND_BLOCK_DIFF, at),
                        Place::Clause => GO,
                    };
                }
                self.row(at::const_expr(op), at)?;
                for &arg in args {
                    self.constant(cx, arg, place, nested, depth + 1, at)?;
                }
                GO
            }
        }
    }

    // ---- the module pass ----

    /// The module pass: properties and flags, module-level tables,
    /// producer records, then the symbols in id order (§8.8).
    pub(crate) fn module(&mut self) -> Flow {
        let module = self.module;
        let at = At::MODULE;
        if self.support.arch() != module.target().arch() {
            self.rule(rules::TARGET_MISMATCH, at)?;
        }
        let props = &module.props;
        if props.pic != PicLevel::None {
            self.row(at::MODULE_PIC, at)?;
        }
        if props.pie != PicLevel::None {
            self.row(at::MODULE_PIE, at)?;
        }
        if let Some(model) = props.code_model {
            self.row(at::MODULE_CODE_MODEL + model as usize, at)?;
        }
        if props.large_data_threshold.is_some() {
            self.row(at::MODULE_LARGE_DATA, at)?;
        }
        if props.uwtable != UwTable::None {
            self.row(at::MODULE_UWTABLE, at)?;
        }
        if props.frame_pointer != FramePointer::None {
            self.row(at::MODULE_FRAME_POINTER, at)?;
        }
        let protection = props.branch_protection.bits();
        if protection != 0 {
            for bit in 0..8 {
                if protection >> bit & 1 != 0 {
                    self.row(at::MODULE_PROTECTION + bit, at)?;
                }
            }
        }
        if props.dwarf_version.is_some() || props.debug_info_version.is_some() {
            self.row(at::DEBUG_MODULE_FLAGS, at)?;
        }
        // `llc` writes the version the flag asks for, and DWARF 4 when
        // there is none; `hod_dwarf` writes DWARF 5 (WP-087).
        if props.dwarf_version != Some(5) && writes_debug_info(module) {
            self.row(at::DEBUG_DWARF_VERSION, at)?;
        }
        for flag in module.other_flags() {
            if !flag.ignorable {
                self.sink.report(Finding::Flag(flag.name), at)?;
            }
        }
        if !module.module_asm().is_empty() {
            // §2.3: directives only, or text an assembler must read.
            let mut effects = Vec::new();
            match crate::asm_text::module_asm_effects(module, &mut effects) {
                Ok(()) => self.row(at::MODULE_ASM_SYMBOLS, at)?,
                Err(_) => self.row(at::MODULE_ASM, at)?,
            }
        }
        if !module.linker_options().is_empty() {
            self.row(at::MODULE_LINKER_OPTIONS, at)?;
        }
        if !module.dependent_libraries().is_empty() {
            self.row(at::MODULE_DEPENDENT_LIBRARIES, at)?;
        }
        for (list, row) in [(module.ctors(), at::MODULE_CTORS), (module.dtors(), at::MODULE_DTORS)]
        {
            for entry in list {
                let valid = entry.func.index() < module.symbol_count();
                let at = At { symbol: valid.then_some(entry.func), inst: None };
                self.row(row, at)?;
                if entry.priority != u16::MAX {
                    self.row(at::STRUCTOR_PRIORITY, at)?;
                }
                if entry.associated.is_some() {
                    self.row(at::STRUCTOR_ASSOCIATED, at)?;
                }
                self.symbol_use(entry.func, None, at)?;
                if let Some(associated) = entry.associated {
                    self.symbol_use(associated, None, at)?;
                }
            }
        }
        for record in module.unsupported() {
            let symbol = record.symbol.filter(|sym| sym.index() < module.symbol_count());
            self.sink.report(Finding::Record(record.construct), At { symbol, inst: None })?;
        }
        for symbol in module.symbols() {
            self.symbol(symbol)?;
        }
        GO
    }

    fn symbol(&mut self, symbol: Symbol<'_>) -> Flow {
        let module = self.module;
        let at = At::symbol(symbol.id());
        let kind = symbol.kind();
        let row = match kind {
            SymbolKind::Unresolved => return self.invalid(rules::UNRESOLVED_SYMBOL, at),
            SymbolKind::Variable { init: None, .. } => at::VARIABLE_DECLARATION,
            SymbolKind::Variable { init: Some(_), .. } => at::VARIABLE_DEFINITION,
            SymbolKind::Function { has_body: false, .. } => at::FUNCTION_DECLARATION,
            SymbolKind::Function { has_body: true, .. } => at::FUNCTION_DEFINITION,
            SymbolKind::Alias { .. } => at::ALIAS,
            SymbolKind::IFunc { .. } => at::IFUNC,
        };
        self.row(row, at)?;
        self.row(at::LINKAGE + symbol.linkage() as usize, at)?;
        match symbol.visibility() {
            Visibility::Default => {}
            other => self.row(at::VISIBILITY + other as usize - 1, at)?,
        }
        match symbol.tls() {
            TlsModel::None => {}
            model => self.row(at::TLS + model as usize - 1, at)?,
        }
        if symbol.addr_space() != 0 {
            let function = matches!(kind, SymbolKind::Function { .. });
            self.rule(
                if function { rules::ADDRSPACE_FUNCTION } else { rules::ADDRSPACE_GLOBAL },
                at,
            )?;
        }
        if symbol.is_definition() {
            if symbol.unnamed_addr() == UnnamedAddr::Global {
                self.row(at::UNNAMED_ADDR, at)?;
            }
            if symbol.section().is_some() {
                self.row(at::SECTION, at)?;
            }
            if let Some(comdat) = symbol.comdat() {
                if (comdat.index()) < module.comdat_count() {
                    self.row(at::COMDAT + module.comdat(comdat).kind as usize, at)?;
                } else {
                    self.invalid(rules::SYMBOL, at)?;
                }
            }
            if symbol.align().is_some() {
                self.row(at::ALIGN, at)?;
            }
            if symbol.is_used() {
                self.row(at::USED, at)?;
            }
        }
        match kind {
            SymbolKind::Variable { init: Some(init), .. } => {
                self.row(at::INIT_DATA, at)?;
                if init.index() >= module.consts.len() {
                    return self.invalid(rules::CONSTANT, at);
                }
                self.constant(None, init, Place::Init, false, 0, at)
            }
            SymbolKind::Function { attrs, personality, .. } => {
                if attrs.index() >= module.fn_attrs_count() {
                    self.invalid(rules::FN_ATTRS, at)?;
                }
                match personality {
                    Some(personality) => self.symbol_use(personality, None, at),
                    None => GO,
                }
            }
            SymbolKind::Alias { target, .. } => self.alias_target(target, at),
            SymbolKind::IFunc { resolver } => self.symbol_use(resolver, None, at),
            SymbolKind::Unresolved | SymbolKind::Variable { init: None, .. } => GO,
        }
    }

    fn alias_target(&mut self, target: ConstId, at: At) -> Flow {
        let module = self.module;
        if target.index() >= module.consts.len() {
            return self.invalid(rules::CONSTANT, at);
        }
        match module.consts.get(target) {
            Const::Addr { sym, addend } => {
                self.symbol_use(sym, None, at)?;
                if addend == 0 { GO } else { self.row(at::ALIAS_OFFSET, at) }
            }
            Const::AddrOfInt { .. } | Const::Null(_) => self.row(at::ALIAS_ABSOLUTE, at),
            Const::Expr { .. } => self.constant(None, target, Place::Clause, false, 0, at),
            _ => self.invalid(rules::INITIALIZER, at),
        }
    }

    // ---- the body pass ----

    /// The body pass: the function's signature and attributes, then the
    /// instructions in layout order (§8.8).
    pub(crate) fn body(&mut self, body: &Body) -> Flow {
        let module = self.module;
        let sym = body.sym;
        if self.support.arch() != module.target().arch() {
            self.rule(rules::TARGET_MISMATCH, At::MODULE)?;
        }
        // A body that is not its symbol's is not walked: nothing in it
        // can be trusted to mean what it says.
        if sym.index() >= module.symbol_count() {
            let _ = self.invalid(rules::BODY_SYMBOL, At::MODULE);
            return ControlFlow::Break(());
        }
        let at = At::symbol(sym);
        let symbol = module.symbol(sym);
        let SymbolKind::Function {
            cc, attrs, ret_attrs, param_attrs, personality, subprogram, ..
        } = symbol.kind()
        else {
            let _ = self.invalid(rules::BODY_NOT_FUNCTION, at);
            return ControlFlow::Break(());
        };
        let fn_ty = body.fn_ty;
        let signature = (fn_ty.index() < module.types.len() && symbol.ty() == fn_ty)
            .then(|| module.types.kind(fn_ty));
        let Some(TypeKind::Function { params, variadic, .. }) = signature else {
            let _ = self.invalid(rules::BODY_TYPE, at);
            return ControlFlow::Break(());
        };
        if body.arg_count as usize != params.len() {
            let _ = self.invalid(rules::BODY, at);
            return ControlFlow::Break(());
        }
        if attrs.index() >= module.fn_attrs_count() {
            let _ = self.invalid(rules::FN_ATTRS, at);
            return ControlFlow::Break(());
        }
        let cx = Cx { body, sym, symbol, params };

        self.cc(cc, at)?;
        if variadic {
            self.row(at::FUNCTION_VARIADIC, at)?;
        }
        if !params.is_empty() {
            self.row(at::FUNCTION_PARAMETERS, at)?;
            let classes = self.classes(at::FUNCTION_PARAMETERS);
            for &param in params {
                self.subject(at::FUNCTION_PARAMETERS, classes, param, at)?;
            }
        }
        self.param_attrs(ret_attrs, at)?;
        for &param in param_attrs {
            self.param_attrs(param, at)?;
        }
        self.fn_attrs(module.fn_attrs(attrs), at)?;
        if let Some(personality) = personality {
            self.symbol_use(personality, None, at)?;
            self.row(at::FUNCTION_PERSONALITY, at)?;
        }
        if subprogram.is_some() {
            self.row(at::DEBUG_SUBPROGRAM, at)?;
        }
        if !body.slots.is_empty() {
            self.row(at::FUNCTION_SLOTS, at)?;
            if body.slots.iter().any(|slot| slot.align() > 16) {
                self.row(at::FUNCTION_OVER_ALIGNED_SLOT, at)?;
            }
        }
        if !body.slot_vars.is_empty() {
            self.row(at::DEBUG_SLOT_VARIABLES, at)?;
        }
        if body.has_locs() {
            self.row(at::DEBUG_LOCATIONS, at)?;
        }

        for &block in &body.layout {
            let range = body.blocks.get(block.index()).map(|data| data.first..data.end);
            let Some(range) = range.filter(|range| range.end as usize <= body.insts.len()) else {
                self.invalid(rules::BODY, at)?;
                continue;
            };
            for raw in range {
                let (id, data) = (InstId::new(raw), &body.insts[raw as usize]);
                if self.accepts(&cx, id, data) {
                    debug_assert!(!self.refuses(&cx, id, data), "the short way accepted {id:?}");
                    continue;
                }
                self.inst(&cx, id, data)?;
            }
        }
        GO
    }

    /// The attributes of a function that has a body.
    fn fn_attrs(&mut self, attrs: FnAttrsView<'_>, at: At) -> Flow {
        let flags = attrs.flags();
        if flags & self.support.fn_flags_off != 0 {
            for bit in 0..FnAttrs::FLAG_NAMES.len() {
                if flags >> bit & 1 != 0 {
                    self.row(at::ATTR_FLAG + bit, at)?;
                }
            }
        }
        if flags & FnAttrs::NOUNWIND == 0 {
            self.row(at::FUNCTION_MAY_UNWIND, at)?;
        }
        if attrs.uwtable() != UwTable::None {
            self.row(at::ATTR_UWTABLE, at)?;
        }
        if attrs.stack_align().is_some() {
            self.row(at::ATTR_ALIGNSTACK, at)?;
        }
        for (key, _) in attrs.strings() {
            self.row(at::ATTR_STRING + key as usize, at)?;
        }
        GO
    }

    // ---- the short way (§8.11) ----
    //
    // Nearly every instruction of an accepted body is an ordinary one
    // with ordinary operands. `accepts` decides those with a few loads
    // and no call; whatever it does not accept goes the long way through
    // `inst`, which is the definition. `accepts` must never say yes
    // where `inst` has a finding: debug builds check that on every
    // instruction.

    /// Whether instruction `id` is accepted, decided the short way.
    /// `false` decides nothing.
    #[inline]
    fn accepts(&self, cx: &Cx<'_>, id: InstId, data: &InstData) -> bool {
        let op = data.op;
        let have = self.support.mask[at::INST + op as usize];
        if have == 0 {
            return false;
        }
        let classes = have & self.support.types;
        match op {
            Opcode::Call => return self.accepts_call(cx, id, data, classes),
            Opcode::Load | Opcode::Store => {
                let mem = MemFlags::from_raw(data.flags);
                if mem.is_volatile() || mem.ordering().is_some() {
                    return false;
                }
            }
            // These have rows and rules of their own.
            Opcode::CmpXchg
            | Opcode::AtomicRmw
            | Opcode::LandingPad
            | Opcode::Switch
            | Opcode::Invoke
            | Opcode::CallBr => return false,
            _ => {}
        }
        let types = &self.module.types;
        (data.ty == TypeId::VOID || has_class(classes, class_code(types, data.ty)))
            && cx
                .body
                .inst(id)
                .operands()
                .iter()
                .all(|&value| self.accepts_operand(cx, classes, value))
    }

    /// Whether `value` is accepted as an operand of an instruction whose
    /// row has `classes`, decided the short way: an instruction's
    /// result, a parameter, a stack slot, a constant that is plain data,
    /// or a symbol that is not thread-local.
    #[inline]
    fn accepts_operand(&self, cx: &Cx<'_>, classes: u16, value: Value) -> bool {
        let module = self.module;
        let ptr = has_class(classes, TypeClass::Ptr as u8);
        let ty = match value.kind() {
            ValueKind::Inst(id) => cx.body.insts.get(id.index()).map(|def| def.ty),
            ValueKind::Arg(index) => cx.params.get(index as usize).copied(),
            ValueKind::Const(id) => module.consts.leaf_type(id),
            ValueKind::Slot(slot) => {
                return ptr
                    && slot.index() < cx.body.slots.len()
                    && self.support.mask[at::OPERAND_SLOT] != 0;
            }
            ValueKind::Sym(sym) => {
                return ptr
                    && self.support.mask[at::OPERAND_SYMBOL] != 0
                    && sym.index() < module.symbol_count()
                    && module.symbol(sym).tls() == TlsModel::None;
            }
            _ => None,
        };
        ty.is_some_and(|ty| has_class(classes, class_code(&module.types, ty)))
    }

    /// The same for a plain direct call: of a function that exists, with
    /// the C convention and no marker, bundle or `returns_twice`.
    fn accepts_call(&self, cx: &Cx<'_>, id: InstId, data: &InstData, classes: u16) -> bool {
        let (module, support) = (self.module, self.support);
        let Some(site) = cx.body.inst(id).call() else { return false };
        let ValueKind::Sym(sym) = site.callee_value().kind() else { return false };
        if sym.index() >= module.symbol_count()
            || site.intrinsic().is_some()
            || site.tail() != TailKind::None
            || site.returns_twice()
            || site.bundle_count() != 0
            || site.cc() != CallConv::C
            || support.mask[CCC] == 0
        {
            return false;
        }
        let callee = module.symbol(sym);
        if callee.tls() != TlsModel::None || self.returns_twice(callee) {
            return false;
        }
        let fn_ty = site.fn_type();
        let fixed = fn_ty.index() < module.types.len()
            && match module.types.kind(fn_ty) {
                TypeKind::Function { variadic, .. } => {
                    !variadic || support.mask[at::CALL_VARIADIC] != 0
                }
                _ => false,
            };
        if !fixed
            || !(data.ty == TypeId::VOID || has_class(classes, class_code(&module.types, data.ty)))
        {
            return false;
        }
        // The attributes the call passes with (`spec/codegen.md` §7.1).
        let declared = declared_attrs(module, &site);
        let has_attrs = site.has_param_attrs() || declared.is_some();
        let accepts = |own: ParamAttrs, theirs: ParamAttrs| {
            merge_call_attrs(own, theirs).is_ok_and(|attrs| self.accepts_attrs(attrs))
        };
        let (ret_declared, of_param) = match declared {
            Some((ret, params)) => (ret, params),
            None => (ParamAttrs::NONE, &[][..]),
        };
        if has_attrs && !accepts(site.ret_attrs(), ret_declared) {
            return false;
        }
        site.args().iter().enumerate().all(|(index, &arg)| {
            let theirs = of_param.get(index).copied().unwrap_or(ParamAttrs::NONE);
            self.accepts_operand(cx, classes, arg)
                && (!has_attrs || accepts(site.arg_attrs(index), theirs))
        })
    }

    /// Whether the parameter attributes `attrs` are all supported.
    fn accepts_attrs(&self, attrs: ParamAttrs) -> bool {
        attrs.is_none() || !self.finds(|walker| walker.param_attrs_slow(attrs, false, At::MODULE))
    }

    /// Whether `walk` has a finding.
    fn finds(&self, walk: impl FnOnce(&mut Walker<'_, Probe>) -> Flow) -> bool {
        let mut probe = Probe::default();
        let (module, support, stack_args) = (self.module, self.support, self.stack_args);
        let _ = walk(&mut Walker { module, support, stack_args, sink: &mut probe });
        probe.0
    }

    /// Whether the long way has a finding at instruction `id`.
    fn refuses(&self, cx: &Cx<'_>, id: InstId, data: &InstData) -> bool {
        self.finds(|walker| walker.inst(cx, id, data))
    }

    // ---- the long way ----

    fn inst(&mut self, cx: &Cx<'_>, id: InstId, data: &InstData) -> Flow {
        let at = At { symbol: Some(cx.sym), inst: Some(id) };
        let op = data.op;
        if op.is_call() {
            return self.call(cx, id, data, at);
        }
        let row = at::INST + op as usize;
        let have = self.support.mask[row];
        if have == 0 {
            self.sink.report(Finding::Row(row), at)?;
        }
        let classes = have & self.support.types;
        let mut far = rules::ADDRSPACE_POINTER;
        match op {
            Opcode::Load => {
                let mem = MemFlags::from_raw(data.flags);
                self.memory(mem, at::MEM_ATOMIC_LOAD, data.ty, at)?;
            }
            Opcode::Store => {
                let mem = MemFlags::from_raw(data.flags);
                if mem.is_volatile() || mem.ordering().is_some() {
                    let stored = cx.type_of(self.module, data.ops[0]);
                    self.memory(mem, at::MEM_ATOMIC_STORE, stored, at)?;
                }
            }
            Opcode::CmpXchg => {
                let (mem, ..) = MemFlags::unpack_cmpxchg(data.flags);
                self.atomic(mem, cx.type_of(self.module, data.ops[1]), at)?;
            }
            Opcode::AtomicRmw => {
                self.row(at::ATOMICRMW + RmwOp::from_raw(data.sub) as usize, at)?;
                let mem = MemFlags::from_raw(data.flags);
                self.atomic(mem, cx.type_of(self.module, data.ops[1]), at)?;
            }
            Opcode::Alloca => far = rules::ADDRSPACE_ALLOCA,
            Opcode::LandingPad => self.landing_pad(cx, data, at)?,
            Opcode::Switch => self.switch_cases(cx, id, at)?,
            _ => {}
        }
        if data.ty != TypeId::VOID {
            let code = class_code(&self.module.types, data.ty);
            if !has_class(classes, code) {
                self.subject_slow(row, code, far, at)?;
            }
        }
        for &value in cx.body.inst(id).operands() {
            self.operand(cx, row, classes, value, at)?;
        }
        GO
    }

    /// The memory-access rows of a `load` or `store` of a `ty`.
    #[inline]
    fn memory(&mut self, mem: MemFlags, atomic_row: usize, ty: TypeId, at: At) -> Flow {
        if !mem.is_volatile() && mem.ordering().is_none() {
            return GO;
        }
        if mem.is_volatile() {
            self.row(at::MEM_VOLATILE, at)?;
        }
        if mem.ordering().is_some() {
            self.row(atomic_row, at)?;
            self.atomic_type(ty, at)?;
        }
        GO
    }

    /// The same for the two instructions that are always atomic.
    fn atomic(&mut self, mem: MemFlags, ty: TypeId, at: At) -> Flow {
        if mem.is_volatile() {
            self.row(at::MEM_VOLATILE, at)?;
        }
        self.atomic_type(ty, at)
    }

    fn atomic_type(&mut self, ty: TypeId, at: At) -> Flow {
        if class_code(&self.module.types, ty) == TypeClass::ScalableVector as u8 {
            self.rule(rules::ATOMIC_SCALABLE, at)
        } else {
            GO
        }
    }

    fn landing_pad(&mut self, cx: &Cx<'_>, data: &InstData, at: At) -> Flow {
        let body = cx.body;
        let Some(pad) = body.pads.get(data.ops[0].bits() as usize) else {
            return self.invalid(rules::BODY, at);
        };
        if pad.cleanup {
            self.row(at::LANDINGPAD_CLEANUP, at)?;
        }
        let clauses = body
            .clauses
            .get(pad.clauses as usize..)
            .and_then(|rest| rest.get(..pad.nclauses as usize))
            .unwrap_or(&[]);
        for &clause in clauses {
            let (row, constant) = match clause {
                Clause::Catch(constant) => (at::LANDINGPAD_CATCH, constant),
                Clause::Filter(constant) => (at::LANDINGPAD_FILTER, constant),
            };
            self.row(row, at)?;
            if constant.index() >= self.module.consts.len() {
                self.invalid(rules::CONSTANT, at)?;
            } else {
                self.constant(Some(cx), constant, Place::Clause, false, 0, at)?;
            }
        }
        GO
    }

    fn switch_cases(&mut self, cx: &Cx<'_>, id: InstId, at: At) -> Flow {
        if let crate::body::InstKind::Switch(switch) = cx.body.inst(id).kind() {
            for (case, _) in switch.cases() {
                if case.index() >= self.module.consts.len() {
                    self.invalid(rules::CONSTANT, at)?;
                }
            }
        }
        GO
    }

    /// One value operand of the instruction whose row is `row`.
    #[inline]
    fn operand(&mut self, cx: &Cx<'_>, row: usize, classes: u16, value: Value, at: At) -> Flow {
        match value.kind() {
            ValueKind::Inst(id) => match cx.body.insts.get(id.index()) {
                Some(def) => self.subject(row, classes, def.ty, at),
                None => self.invalid(rules::BODY, at),
            },
            ValueKind::Arg(index) => match cx.params.get(index as usize) {
                Some(&ty) => self.subject(row, classes, ty, at),
                None => self.invalid(rules::BODY, at),
            },
            ValueKind::Const(id) => self.const_operand(cx, row, classes, id, at),
            ValueKind::Sym(sym) => {
                // A thread-local symbol's address is an operand like any
                // other (WP-073); only a callee may not be one.
                self.symbol_use(sym, None, at)?;
                self.row(at::OPERAND_SYMBOL, at)?;
                self.subject(row, classes, TypeId::PTR, at)
            }
            ValueKind::Slot(slot) => {
                if slot.index() >= cx.body.slots.len() {
                    self.invalid(rules::BODY, at)?;
                }
                self.row(at::OPERAND_SLOT, at)?;
                self.subject(row, classes, TypeId::PTR, at)
            }
            ValueKind::Asm(_) => self.invalid(rules::INLINE_ASM, at),
            ValueKind::Meta(_) | ValueKind::None => self.invalid(rules::BODY, at),
        }
    }

    fn const_operand(
        &mut self,
        cx: &Cx<'_>,
        row: usize,
        classes: u16,
        id: ConstId,
        at: At,
    ) -> Flow {
        let module = self.module;
        if id.index() >= module.consts.len() {
            return self.invalid(rules::CONSTANT, at);
        }
        let ty = if let Some(ty) = module.consts.leaf_type(id) {
            ty
        } else {
            self.constant(Some(cx), id, Place::Operand, false, 0, at)?;
            module.consts.ty(id)
        };
        self.subject(row, classes, ty, at)
    }

    fn call(&mut self, cx: &Cx<'_>, id: InstId, data: &InstData, at: At) -> Flow {
        let Some(site) = cx.body.inst(id).call() else {
            return self.invalid(rules::BODY, at);
        };
        let intrinsic = site.intrinsic();
        let mut row = at::INST + data.op as usize;
        // A plain call of an intrinsic is the intrinsic and nothing else.
        if data.op != Opcode::Call || intrinsic.is_none() {
            self.row(row, at)?;
        }
        let mut direct = None;
        let mut callee_is_operand = false;
        let mut tls_address = false;
        match intrinsic {
            Some(Intrinsic::Other(name)) => {
                self.sink.report(Finding::Named("intrinsic:", name), at)?;
                row = at::ANY;
            }
            Some(known) => {
                row = at::INTRINSIC + usize::from(known.code()) - 1;
                self.row(row, at)?;
                tls_address = known == Intrinsic::ThreadLocalAddress;
            }
            None => (direct, callee_is_operand) = self.callee(&site, at)?,
        }
        match site.tail() {
            TailKind::None => {}
            TailKind::Tail => self.row(at::TAIL_TAIL, at)?,
            TailKind::NoTail => self.row(at::TAIL_NOTAIL, at)?,
            TailKind::MustTail => self.musttail(cx, &site, at)?,
        }
        if site.returns_twice() || direct.is_some_and(|callee| self.returns_twice(callee)) {
            self.row(at::ATTR_RETURNS_TWICE, at)?;
        }
        let has_bundles = site.bundle_count() != 0;
        if has_bundles {
            for bundle in site.bundles() {
                match bundle_row(bundle.tag) {
                    Some(row) => self.row(row, at)?,
                    None => self.sink.report(Finding::Named("bundle:", bundle.tag), at)?,
                }
            }
        }
        // The attributes the call passes with (`spec/codegen.md` §7.1):
        // its own and, for a direct call through the callee's own type,
        // the declaration's.
        let declared = declared_attrs(self.module, &site);
        let has_attrs = site.has_param_attrs() || declared.is_some();
        let (ret_declared, of_param) = match declared {
            Some((ret, params)) => (ret, params),
            None => (ParamAttrs::NONE, &[][..]),
        };
        if has_attrs {
            self.call_attrs(site.ret_attrs(), ret_declared, at)?;
        }
        let classes = self.classes(row);
        if data.ty != TypeId::VOID {
            self.subject(row, classes, data.ty, at)?;
        }
        if callee_is_operand {
            self.operand(cx, row, classes, site.callee_value(), at)?;
        }
        let asm_callee = matches!(site.callee_value().kind(), ValueKind::Asm(_));
        for (index, &arg) in site.args().iter().enumerate() {
            match arg.kind() {
                // The one place a thread-local symbol may stand.
                ValueKind::Sym(sym) if tls_address && index == 0 => {
                    self.symbol_use(sym, None, at)?;
                    self.subject(row, classes, TypeId::PTR, at)?;
                }
                // A metadata operand (§5.3) is the intrinsic's business:
                // its row and its target judge what it names.
                ValueKind::Meta(meta)
                    if site.intrinsic().is_some() && meta.index() < cx.body.meta_count() => {}
                _ => self.operand(cx, row, classes, arg, at)?,
            }
            if has_attrs {
                let theirs = of_param.get(index).copied().unwrap_or(ParamAttrs::NONE);
                let own = site.arg_attrs(index);
                if asm_callee && theirs.is_none() {
                    self.param_attrs_slow(own, true, at)?;
                } else {
                    self.call_attrs(own, theirs, at)?;
                }
            }
        }
        if has_bundles {
            for bundle in site.bundles() {
                for &input in bundle.inputs {
                    self.operand(cx, row, classes, input, at)?;
                }
            }
        }
        GO
    }

    /// The rows of the attributes a call passes one argument (or its
    /// result) with: `own` merged with the declaration's `theirs`. A
    /// conflict, which the call sequence refuses, reports both.
    fn call_attrs(&mut self, own: ParamAttrs, theirs: ParamAttrs, at: At) -> Flow {
        if let Ok(attrs) = merge_call_attrs(own, theirs) {
            self.param_attrs(attrs, at)
        } else {
            self.param_attrs(own, at)?;
            self.param_attrs(theirs, at)
        }
    }

    /// What a call of something other than an intrinsic calls, how, and
    /// with which convention. Returns the function called directly, if
    /// any, and whether the callee is an operand like the arguments.
    fn callee(
        &mut self,
        site: &CallSite<'_>,
        at: At,
    ) -> ControlFlow<(), (Option<Symbol<'a>>, bool)> {
        let module = self.module;
        let mut direct = None;
        let mut is_operand = false;
        match site.callee_value().kind() {
            ValueKind::Sym(sym) => {
                self.symbol_use(sym, Some(rules::TLS_OPERAND), at)?;
                if sym.index() < module.symbol_count() {
                    direct = Some(module.symbol(sym));
                }
            }
            ValueKind::Asm(asm) => {
                if asm.index() >= module.asm_count() {
                    self.invalid(rules::INLINE_ASM, at)?;
                }
                // §2.3: a template that assembles to nothing needs no
                // assembler.
                let empty = asm.index() < module.asm_count()
                    && crate::asm_text::asm_template_is_empty(
                        self.support.arch(),
                        module.asm(asm).template,
                    );
                if empty {
                    self.row(at::CALL_INLINE_ASM_EMPTY, at)?;
                } else {
                    self.row(at::CALL_INLINE_ASM, at)?;
                }
            }
            _ => {
                self.row(at::CALL_INDIRECT, at)?;
                is_operand = true;
            }
        }
        let fn_ty = site.fn_type();
        let signature = (fn_ty.index() < module.types.len()).then(|| module.types.kind(fn_ty));
        match signature {
            Some(TypeKind::Function { variadic, .. }) => {
                if variadic {
                    self.row(at::CALL_VARIADIC, at)?;
                }
            }
            _ => self.invalid(rules::TYPE, at)?,
        }
        self.cc(site.cc(), at)?;
        ControlFlow::Continue((direct, is_operand))
    }

    /// Whether the function `callee` is declared `returns_twice`.
    fn returns_twice(&self, callee: Symbol<'_>) -> bool {
        match callee.kind() {
            SymbolKind::Function { attrs, .. } if attrs.index() < self.module.fn_attrs_count() => {
                self.module.fn_attrs(attrs).has(FnAttrs::RETURNS_TWICE)
            }
            _ => false,
        }
    }

    /// The structural rules of a `musttail` call (§8.6), then its row.
    fn musttail(&mut self, cx: &Cx<'_>, site: &CallSite<'_>, at: At) -> Flow {
        let cc = site.cc();
        if cc == CallConv::TAIL || cc == CallConv::SWIFT_TAIL {
            self.rule(rules::MUSTTAIL_CALLEE_POP, at)?;
        }
        let areas = self.stack_args.and_then(|hook| {
            Some((hook.of_function(self.module, cx.symbol)?, hook.of_call(self.module, site)?))
        });
        match areas {
            Some((have, need)) if need > have => self.rule(rules::MUSTTAIL_GROW, at)?,
            Some(_) => {}
            None => self.rule(rules::MUSTTAIL_UNKNOWN, at)?,
        }
        self.row(at::TAIL_MUSTTAIL, at)
    }
}
