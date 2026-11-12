//! The printer (`spec/ir.md` §9): LLVM textual IR from a module and its
//! function bodies.
//!
//! The text is a normal form, not a copy of what a producer read: `PtrAdd`
//! prints as `i8` `getelementptr`s with explicit index arithmetic, stack
//! slots as entry-block `alloca`s, values and blocks are named after their
//! ids, and what the IR dropped stays dropped. `llvm-as` of the chosen
//! major accepts it, the verifier passes it, and `llc` compiles it to code
//! that behaves as the IR says.
//!
//! This file holds the public API and what both halves share: names,
//! strings, types and constants. `module.rs` prints what a module owns,
//! `body.rs` prints a function body.

mod body;
mod debug;
mod module;
#[cfg(test)]
mod random_tests;

use std::collections::BTreeMap;
use std::fmt::{self, Write};
use std::io;

use crate::body::Body;
use crate::constant::{Const, ConstOp};
use crate::ids::{ConstId, DiId, SymId, TypeId};
use crate::module::Module;
use crate::symbol::{Linkage, SymbolKind};
use crate::types::TypeKind;

/// An LLVM major version whose text the printer writes: 19 to 22.
///
/// What each one changes is the table of `spec/ir.md` §9.2.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Dialect(u8);

impl Dialect {
    /// LLVM 19.
    pub const LLVM19: Dialect = Dialect(19);
    /// LLVM 20.
    pub const LLVM20: Dialect = Dialect(20);
    /// LLVM 21.
    pub const LLVM21: Dialect = Dialect(21);
    /// LLVM 22.
    pub const LLVM22: Dialect = Dialect(22);
    /// Every dialect, oldest first.
    pub const ALL: [Dialect; 4] =
        [Dialect::LLVM19, Dialect::LLVM20, Dialect::LLVM21, Dialect::LLVM22];

    /// The dialect of LLVM `major`.
    ///
    /// # Errors
    /// `Unsupported` (`dialect:llvm-<major>`) outside 19 to 22.
    pub fn new(major: u32) -> Result<Dialect, PrintError> {
        match major {
            19..=22 => Ok(Dialect(major as u8)),
            _ => Err(PrintError::Unsupported {
                construct: format!("dialect:llvm-{major}"),
                symbol: None,
            }),
        }
    }

    /// The LLVM major.
    #[inline]
    pub const fn major(self) -> u32 {
        self.0 as u32
    }
}

/// Which share of a module [`Module::print_part`] writes (§9.6).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Part {
    /// The module as it is. A function whose body is not supplied is
    /// declared.
    Whole,
    /// One of the two objects a module is split into: everything `Whole`
    /// has, with local symbols made `hidden` so that the other object can
    /// name them.
    Main,
    /// The other object: only the functions whose bodies are supplied are
    /// defined, and every other symbol is declared.
    Rest,
}

/// Why the printer wrote no text, or stopped writing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PrintError {
    /// The dialect has no text for a construct, or the IR does not keep
    /// what the text would need. `construct` is a stable name
    /// (`const-expr:mul`, `inst:catchswitch`, `attr:sanitize_type`).
    Unsupported {
        /// The construct.
        construct: String,
        /// The symbol being printed when it was met.
        symbol: Option<SymId>,
    },
    /// The module or a body is not well formed: an id that names nothing,
    /// a body for a symbol that is no function. A body that passes
    /// `Body::verify` in a module that passes `Module::verify_symbols`
    /// does not produce this.
    Malformed {
        /// What was found.
        what: String,
        /// The symbol being printed when it was met.
        symbol: Option<SymId>,
    },
    /// The sink refused the text.
    Write,
}

impl fmt::Display for PrintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let at = |f: &mut fmt::Formatter<'_>, symbol: &Option<SymId>| match symbol {
            Some(symbol) => write!(f, " (in {symbol:?})"),
            None => Ok(()),
        };
        match self {
            PrintError::Unsupported { construct, symbol } => {
                write!(f, "cannot print `{construct}`")?;
                at(f, symbol)
            }
            PrintError::Malformed { what, symbol } => {
                write!(f, "malformed IR: {what}")?;
                at(f, symbol)
            }
            PrintError::Write => f.write_str("the output refused the text"),
        }
    }
}

impl std::error::Error for PrintError {}

impl From<fmt::Error> for PrintError {
    fn from(_: fmt::Error) -> Self {
        PrintError::Write
    }
}

/// An [`io::Write`] as the [`fmt::Write`] the printer writes to.
///
/// The printer makes many small writes: give it a buffered writer.
#[derive(Debug)]
pub struct IoSink<W: io::Write> {
    inner: W,
    error: Option<io::Error>,
}

impl<W: io::Write> IoSink<W> {
    /// Wraps `inner`.
    pub fn new(inner: W) -> IoSink<W> {
        IoSink { inner, error: None }
    }

    /// The writer back, or the error that made the printer stop with
    /// [`PrintError::Write`].
    pub fn finish(self) -> io::Result<W> {
        match self.error {
            Some(error) => Err(error),
            None => Ok(self.inner),
        }
    }
}

impl<W: io::Write> Write for IoSink<W> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.inner.write_all(text.as_bytes()).map_err(|error| {
            self.error = Some(error);
            fmt::Error
        })
    }
}

impl Module {
    /// Writes the module as LLVM text of `dialect` (§9).
    ///
    /// The module does not own bodies: `bodies` supplies them, in any
    /// order. A function for which none is supplied is written as a
    /// declaration.
    ///
    /// # Errors
    /// See [`PrintError`]. The sink may have received part of the text.
    pub fn print<'a, W, I>(
        &'a self,
        out: &mut W,
        dialect: Dialect,
        bodies: I,
    ) -> Result<(), PrintError>
    where
        W: Write + ?Sized,
        I: IntoIterator<Item = &'a Body>,
    {
        self.print_part(out, dialect, Part::Whole, bodies)
    }

    /// Writes one part of the module (§9.6). Printed as `Part::Main` with
    /// some of the bodies and as `Part::Rest` with the others, a module
    /// becomes two texts whose objects link to what the whole module's
    /// object is.
    ///
    /// # Errors
    /// See [`PrintError`].
    pub fn print_part<'a, W, I>(
        &'a self,
        out: &mut W,
        dialect: Dialect,
        part: Part,
        bodies: I,
    ) -> Result<(), PrintError>
    where
        W: Write + ?Sized,
        I: IntoIterator<Item = &'a Body>,
    {
        let mut table: Vec<Option<&Body>> = vec![None; self.symbol_count()];
        for body in bodies {
            let id = body.symbol();
            let malformed = |what: &str| {
                Err(PrintError::Malformed { what: what.to_string(), symbol: Some(id) })
            };
            let Some(entry) = table.get_mut(id.index()) else {
                return malformed("a body for a symbol the module does not have");
            };
            let symbol = self.symbol(id);
            if !matches!(symbol.kind(), SymbolKind::Function { has_body: true, .. }) {
                return malformed("a body for a symbol that is not a function with a body");
            }
            if symbol.ty() != body.fn_type() {
                return malformed("a body whose function type is not its symbol's");
            }
            if entry.replace(body).is_some() {
                return malformed("two bodies for one function");
            }
        }
        let mut writer = Writer::new(out, self, dialect, part);
        writer.bodies = table;
        writer.module()
    }

    /// Writes one function's definition, as it stands in the module's
    /// text, and nothing else: a fragment for reading, which names the
    /// module's symbols, attribute groups and intrinsics without declaring
    /// them.
    ///
    /// # Errors
    /// See [`PrintError`].
    pub fn print_body<W>(
        &self,
        out: &mut W,
        dialect: Dialect,
        body: &Body,
    ) -> Result<(), PrintError>
    where
        W: Write + ?Sized,
    {
        let mut writer = Writer::new(out, self, dialect, Part::Whole);
        writer.alone = true;
        let id = body.symbol();
        writer.cur = Some(id);
        if id.index() >= self.symbol_count() {
            return writer.malformed("a body for a symbol the module does not have");
        }
        let symbol = self.symbol(id);
        if !symbol.is_function() || symbol.ty() != body.fn_type() {
            return writer.malformed("a body whose symbol is not a function of its type");
        }
        writer.function(symbol, symbol, Some(body))
    }
}

/// The result of every writing step.
type R = Result<(), PrintError>;

/// An intrinsic declaration the text needs: the member (0 and the name for
/// `Other`) and its overload types.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct DeclKey<'a> {
    code: u16,
    name: &'a str,
    overloads: [TypeId; 3],
}

/// The state of one print.
struct Writer<'a, W: Write + ?Sized> {
    out: &'a mut W,
    m: &'a Module,
    dialect: Dialect,
    part: Part,
    /// The supplied body of each symbol, by symbol index.
    bodies: Vec<Option<&'a Body>>,
    /// One body is being printed by itself: nothing is known about the
    /// others, and nothing is collected.
    alone: bool,
    /// The symbol being printed, for errors.
    cur: Option<SymId>,
    /// Scratch for an aggregate index path.
    path: Vec<u32>,
    /// The intrinsics the bodies call, with the function type of the call.
    decls: BTreeMap<DeclKey<'a>, TypeId>,
    /// Which attribute sets a printed function names.
    attrs_used: Vec<bool>,
    /// `Part::Rest`: whether each COMDAT has all its members here.
    comdat_whole: Vec<bool>,
    /// The `!dbg` attachments of variables, sorted by symbol.
    global_dbg: Vec<(SymId, DiId)>,
}

impl<'a, W: Write + ?Sized> Writer<'a, W> {
    fn new(out: &'a mut W, m: &'a Module, dialect: Dialect, part: Part) -> Self {
        Writer {
            out,
            m,
            dialect,
            part,
            bodies: Vec::new(),
            alone: false,
            cur: None,
            path: Vec::new(),
            decls: BTreeMap::new(),
            attrs_used: vec![false; m.fn_attrs_count()],
            comdat_whole: Vec::new(),
            global_dbg: Vec::new(),
        }
    }

    fn unsupported<T>(&self, construct: impl Into<String>) -> Result<T, PrintError> {
        Err(PrintError::Unsupported { construct: construct.into(), symbol: self.cur })
    }

    fn malformed<T>(&self, what: impl Into<String>) -> Result<T, PrintError> {
        Err(PrintError::Malformed { what: what.into(), symbol: self.cur })
    }

    #[inline]
    fn s(&mut self, text: &str) -> R {
        self.out.write_str(text)?;
        Ok(())
    }

    // ---- names ----

    /// `@name` of a symbol: its name, or its id when it has none (in a
    /// split, a name both parts agree on).
    fn sym_name(&mut self, id: SymId) -> R {
        if id.index() >= self.m.symbol_count() {
            return self.malformed(format!("{id:?} is not a symbol of the module"));
        }
        let symbol = self.m.symbol(id);
        if !symbol.is_anonymous() {
            return write_name(self.out, '@', symbol.name());
        }
        match self.part {
            Part::Whole => write!(self.out, "@{}", id.raw())?,
            Part::Main | Part::Rest => write!(self.out, "@__hod_anon.{}", id.raw())?,
        }
        Ok(())
    }

    /// A symbol's address as a `ptr` value.
    fn sym_ref(&mut self, id: SymId) -> R {
        if id.index() < self.m.symbol_count() && self.m.symbol(id).addr_space() != 0 {
            // Its address is a `ptr addrspace(N)` to LLVM and a `ptr` to
            // hod_ir.
            return self.unsupported("address-of-symbol-in-address-space");
        }
        self.sym_name(id)
    }

    /// Whether this text defines function `id`.
    fn fn_defined(&self, id: SymId) -> bool {
        self.bodies.get(id.index()).is_some_and(Option::is_some)
    }

    // ---- types ----

    fn ty(&mut self, ty: TypeId) -> R {
        let m = self.m;
        if ty.index() >= m.types.len() {
            return self.malformed(format!("{ty:?} is not a type of the module"));
        }
        match m.types.kind(ty) {
            TypeKind::Void => self.s("void"),
            TypeKind::Label => self.s("label"),
            TypeKind::Metadata => self.s("metadata"),
            TypeKind::Token => self.s("token"),
            TypeKind::Half => self.s("half"),
            TypeKind::BFloat => self.s("bfloat"),
            TypeKind::Float => self.s("float"),
            TypeKind::Double => self.s("double"),
            TypeKind::Fp128 => self.s("fp128"),
            TypeKind::X86Fp80 => self.s("x86_fp80"),
            TypeKind::Int(bits) => Ok(write!(self.out, "i{bits}")?),
            TypeKind::Ptr { addr_space: 0 } => self.s("ptr"),
            TypeKind::Ptr { addr_space } => {
                if !self.addr_space_is_laid_out(addr_space) {
                    // The dialect's data layout gives it another width.
                    return self.unsupported(format!("type:ptr addrspace({addr_space})"));
                }
                Ok(write!(self.out, "ptr addrspace({addr_space})")?)
            }
            TypeKind::Vector { elem, len } => {
                write!(self.out, "<{len} x ")?;
                self.ty(elem)?;
                self.s(">")
            }
            TypeKind::ScalableVector { elem, min_len } => {
                write!(self.out, "<vscale x {min_len} x ")?;
                self.ty(elem)?;
                self.s(">")
            }
            TypeKind::Array { elem, len } => {
                write!(self.out, "[{len} x ")?;
                self.ty(elem)?;
                self.s("]")
            }
            TypeKind::Struct { fields, packed } => {
                self.s(match (packed, fields.is_empty()) {
                    (false, false) => "{ ",
                    (false, true) => "{",
                    (true, false) => "<{ ",
                    (true, true) => "<{",
                })?;
                self.ty_list(fields)?;
                self.s(match (packed, fields.is_empty()) {
                    (false, false) => " }",
                    (false, true) => "}",
                    (true, false) => " }>",
                    (true, true) => "}>",
                })
            }
            TypeKind::Opaque(number) => Ok(write!(self.out, "%hod.opaque.{number}")?),
            TypeKind::Function { ret, params, variadic } => {
                self.ty(ret)?;
                self.s(" (")?;
                self.ty_list(params)?;
                if variadic {
                    self.s(if params.is_empty() { "..." } else { ", ..." })?;
                }
                self.s(")")
            }
            // Only the name is kept, and the parameters are part of the type.
            TypeKind::TargetExt { name } => self.unsupported(format!("type:target(\"{name}\")")),
        }
    }

    fn ty_list(&mut self, types: &[TypeId]) -> R {
        for (at, &ty) in types.iter().enumerate() {
            if at > 0 {
                self.s(", ")?;
            }
            self.ty(ty)?;
        }
        Ok(())
    }

    // ---- constants ----

    fn const_ty(&self, id: ConstId) -> Result<TypeId, PrintError> {
        if id.index() >= self.m.consts.len() {
            return self.malformed(format!("{id:?} is not a constant of the module"));
        }
        Ok(self.m.consts.ty(id))
    }

    /// `type value`.
    fn typed_const(&mut self, id: ConstId) -> R {
        let ty = self.const_ty(id)?;
        self.ty(ty)?;
        self.s(" ")?;
        self.constant(id)
    }

    fn typed_consts(&mut self, ids: &[ConstId]) -> R {
        for (at, &id) in ids.iter().enumerate() {
            if at > 0 {
                self.s(", ")?;
            }
            self.typed_const(id)?;
        }
        Ok(())
    }

    /// A constant's value, without its type.
    #[expect(clippy::too_many_lines, reason = "one arm per kind of constant")]
    fn constant(&mut self, id: ConstId) -> R {
        let m = self.m;
        self.const_ty(id)?;
        match m.consts.get(id) {
            Const::Int { ty, value } => match m.types.int_bits(ty) {
                Some(1) => self.s(if value == 0 { "false" } else { "true" }),
                Some(bits @ 2..=64) => {
                    let shift = 64 - bits;
                    Ok(write!(self.out, "{}", ((value << shift) as i64) >> shift)?)
                }
                _ => self.malformed("an integer constant whose type is not an integer"),
            },
            Const::WideInt { words, .. } => {
                self.s("u0x")?;
                let mut leading = true;
                for &word in words.iter().rev() {
                    if leading && word == 0 {
                        continue;
                    }
                    if leading {
                        write!(self.out, "{word:X}")?;
                    } else {
                        write!(self.out, "{word:016X}")?;
                    }
                    leading = false;
                }
                if leading {
                    self.s("0")?;
                }
                Ok(())
            }
            Const::Float { ty, bits } => match m.types.kind(ty) {
                TypeKind::Half => Ok(write!(self.out, "0xH{bits:04X}")?),
                TypeKind::BFloat => Ok(write!(self.out, "0xR{bits:04X}")?),
                TypeKind::Float => Ok(write!(self.out, "0x{:016X}", f32_as_f64_bits(bits as u32))?),
                TypeKind::Double => Ok(write!(self.out, "0x{bits:016X}")?),
                _ => self.malformed("a float constant whose type is not a float"),
            },
            Const::WideFloat { ty, bits } => {
                let (low, high) = (bits as u64, (bits >> 64) as u64);
                match m.types.kind(ty) {
                    // LLVM writes the low word first.
                    TypeKind::Fp128 => Ok(write!(self.out, "0xL{low:016X}{high:016X}")?),
                    TypeKind::X86Fp80 => Ok(write!(self.out, "0xK{high:04X}{low:016X}")?),
                    _ => self.malformed("a wide float constant whose type is not one"),
                }
            }
            Const::Null(_) => self.s("null"),
            Const::Zero(_) => self.s("zeroinitializer"),
            Const::Undef(_) => self.s("undef"),
            Const::Poison(_) => self.s("poison"),
            Const::NoneToken => self.s("none"),
            Const::Aggregate { ty, elems } => {
                let (open, close) = match m.types.kind(ty) {
                    TypeKind::Struct { packed: false, .. } => ("{ ", " }"),
                    TypeKind::Struct { packed: true, .. } => ("<{ ", " }>"),
                    TypeKind::Array { .. } => ("[", "]"),
                    TypeKind::Vector { .. } => ("<", ">"),
                    _ => return self.malformed("an aggregate constant of another type"),
                };
                self.s(open)?;
                self.typed_consts(elems)?;
                self.s(close)
            }
            Const::Splat { elem, .. } => {
                self.s("splat (")?;
                self.typed_const(elem)?;
                self.s(")")
            }
            Const::Bytes { bytes, .. } => {
                self.s("c\"")?;
                write_escaped(self.out, bytes)?;
                self.s("\"")
            }
            Const::Addr { sym, addend: 0 } => self.sym_ref(sym),
            Const::Addr { sym, addend } => {
                self.s("getelementptr (i8, ptr ")?;
                self.sym_ref(sym)?;
                Ok(write!(self.out, ", i64 {addend})")?)
            }
            Const::BlockAddr { func, block } => {
                if !self.alone {
                    let Some(body) = self.bodies.get(func.index()).copied().flatten() else {
                        return self.unsupported("blockaddress-of-declaration");
                    };
                    if block.index() >= body.block_count() {
                        return self.malformed("a blockaddress of a block the function lacks");
                    }
                    if block == body.entry() {
                        return self.unsupported("blockaddress-of-entry-block");
                    }
                }
                self.s("blockaddress(")?;
                self.sym_ref(func)?;
                Ok(write!(self.out, ", %b{})", block.raw())?)
            }
            Const::SymDiff { ty, a, b, addend, plt } => {
                let Some(bits @ 1..=64) = m.types.int_bits(ty) else {
                    return self.malformed("a symbol difference wider than 64 bits");
                };
                if bits < 64 {
                    self.s("trunc (i64 ")?;
                }
                if addend != 0 {
                    self.s("add (i64 ")?;
                }
                self.s("sub (i64 ptrtoint (ptr ")?;
                if plt {
                    self.s("dso_local_equivalent ")?;
                }
                self.sym_ref(a)?;
                self.s(" to i64), i64 ptrtoint (ptr ")?;
                self.sym_ref(b)?;
                self.s(" to i64))")?;
                if addend != 0 {
                    write!(self.out, ", i64 {addend})")?;
                }
                if bits < 64 {
                    write!(self.out, " to i{bits})")?;
                }
                Ok(())
            }
            Const::IntOfAddr { ty, addr } => {
                self.s("ptrtoint (ptr ")?;
                self.constant(addr)?;
                self.s(" to ")?;
                self.ty(ty)?;
                self.s(")")
            }
            Const::AddrOfInt { value } => Ok(write!(self.out, "inttoptr (i64 {value} to ptr)")?),
            Const::DsoLocalEquivalent(sym) => {
                self.s("dso_local_equivalent ")?;
                self.sym_ref(sym)
            }
            Const::NoCfi(sym) => {
                self.s("no_cfi ")?;
                self.sym_ref(sym)
            }
            Const::Expr { op, ty, source_ty, args } => self.const_expr(op, ty, source_ty, args),
        }
    }

    /// A constant expression that has no normal form (§4.2).
    fn const_expr(
        &mut self,
        op: ConstOp,
        ty: TypeId,
        source_ty: Option<TypeId>,
        args: &[ConstId],
    ) -> R {
        let major = self.dialect.major();
        let mut name = op.name();
        match op {
            // LLVM 21 removed the `mul` expression; the others went before
            // LLVM 19.
            ConstOp::Mul if major <= 20 => {}
            ConstOp::Mul
            | ConstOp::ZExt
            | ConstOp::SExt
            | ConstOp::And
            | ConstOp::Or
            | ConstOp::Shl => return self.unsupported(format!("const-expr:{name}")),
            ConstOp::PtrToAddr if major < 22 => {
                let from = args.first().map(|&arg| self.const_ty(arg)).transpose()?;
                if !from.is_some_and(|from| self.address_is_whole_pointer(from)) {
                    return self.unsupported("const-expr:ptrtoaddr");
                }
                name = "ptrtoint";
            }
            _ => {}
        }
        self.s(name)?;
        self.s(" (")?;
        if op.is_cast() {
            let [arg] = args else {
                return self.malformed("a cast expression without one operand");
            };
            self.typed_const(*arg)?;
            self.s(" to ")?;
            self.ty(ty)?;
        } else {
            if op == ConstOp::GetElementPtr {
                let Some(source) = source_ty else {
                    return self.malformed("a getelementptr expression without a source type");
                };
                self.ty(source)?;
                self.s(", ")?;
            }
            self.typed_consts(args)?;
        }
        self.s(")")
    }

    /// Whether the address of a pointer of type `ty` (or of each pointer of
    /// a vector) is the whole pointer, so that `ptrtoaddr` and `ptrtoint`
    /// to the address width are one operation.
    fn address_is_whole_pointer(&self, ty: TypeId) -> bool {
        let types = &self.m.types;
        let pointer = match types.kind(ty) {
            TypeKind::Vector { elem, .. } | TypeKind::ScalableVector { elem, .. } => elem,
            _ => ty,
        };
        match types.kind(pointer) {
            TypeKind::Ptr { addr_space } => {
                let spec = types.data_layout().pointer(addr_space);
                spec.bits == spec.index_bits
            }
            _ => false,
        }
    }

    // ---- linkage ----

    /// The linkage a symbol is written with: its own, except that a split
    /// makes a local symbol `hidden` and external, and that a declaration
    /// is external unless it is `extern_weak`.
    fn linkage_of(
        &self,
        symbol: crate::symbol::Symbol<'_>,
        define: bool,
    ) -> (Linkage, crate::symbol::Visibility) {
        let (mut linkage, mut visibility) = (symbol.linkage(), symbol.visibility());
        if self.part != Part::Whole && linkage.is_local() {
            linkage = Linkage::External;
            visibility = crate::symbol::Visibility::Hidden;
        }
        if !define && linkage != Linkage::ExternWeak {
            linkage = Linkage::External;
        }
        (linkage, visibility)
    }
}

/// The bit pattern of the `double` equal to the `float` with pattern
/// `bits`, which is how LLVM's text spells a `float`. A NaN keeps its
/// payload and its signaling bit, as LLVM's own printer arranges.
fn f32_as_f64_bits(bits: u32) -> u64 {
    let value = f32::from_bits(bits);
    if value.is_nan() {
        let sign = u64::from(bits >> 31) << 63;
        let payload = u64::from(bits & 0x007f_ffff) << 29;
        return sign | (0x7ff << 52) | payload;
    }
    f64::from(value).to_bits()
}

/// Whether `name` needs no quotes after its sigil: LLVM's identifier
/// characters, not starting with a digit.
fn is_plain_name(name: &[u8]) -> bool {
    let plain =
        |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'$' | b'.' | b'_');
    match name.split_first() {
        Some((&first, rest)) => {
            !first.is_ascii_digit() && plain(first) && rest.iter().all(|&b| plain(b))
        }
        None => false,
    }
}

/// `@name`, `$name` or `%name`, quoted when it must be.
fn write_name<W: Write + ?Sized>(out: &mut W, sigil: char, name: &[u8]) -> R {
    out.write_char(sigil)?;
    if is_plain_name(name) {
        // Plain names are ASCII.
        out.write_str(std::str::from_utf8(name).unwrap_or_default())?;
        Ok(())
    } else {
        write_quoted(out, name)
    }
}

/// `"bytes"`, escaped.
fn write_quoted<W: Write + ?Sized>(out: &mut W, bytes: &[u8]) -> R {
    out.write_char('"')?;
    write_escaped(out, bytes)?;
    out.write_char('"')?;
    Ok(())
}

/// The inside of a quoted string: printable ASCII as it is, `"`, `\` and
/// every other byte as `\XX`.
fn write_escaped<W: Write + ?Sized>(out: &mut W, bytes: &[u8]) -> R {
    let literal = |byte: u8| (0x20..0x7f).contains(&byte) && byte != b'"' && byte != b'\\';
    let mut rest = bytes;
    while !rest.is_empty() {
        let run = rest.iter().position(|&byte| !literal(byte)).unwrap_or(rest.len());
        // A run of printable ASCII is UTF-8.
        out.write_str(std::str::from_utf8(&rest[..run]).unwrap_or_default())?;
        rest = &rest[run..];
        if let Some((&byte, tail)) = rest.split_first() {
            write!(out, "\\{byte:02X}")?;
            rest = tail;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §9.3: a `float` is spelled as the `double` of the same value, and a
    /// NaN keeps its payload.
    #[test]
    fn floats_are_spelled_as_doubles() {
        assert_eq!(f32_as_f64_bits(1.5f32.to_bits()), 1.5f64.to_bits());
        assert_eq!(f32_as_f64_bits((-0.0f32).to_bits()), (-0.0f64).to_bits());
        assert_eq!(f32_as_f64_bits(f32::INFINITY.to_bits()), f64::INFINITY.to_bits());
        assert_eq!(f32_as_f64_bits(1), f64::from(f32::from_bits(1)).to_bits());
        // A signaling NaN with a payload, and a quiet one.
        assert_eq!(f32_as_f64_bits(0x7f80_0001), 0x7ff0_0000_2000_0000);
        assert_eq!(f32_as_f64_bits(0xffc0_0000), 0xfff8_0000_0000_0000);
    }

    /// §9.3: which names are written bare, and how the others are escaped.
    #[test]
    fn names_are_quoted_when_needed() {
        let text = |name: &[u8]| {
            let mut out = String::new();
            write_name(&mut out, '@', name).unwrap();
            out
        };
        assert_eq!(text(b"_ZN3fooEv"), "@_ZN3fooEv");
        assert_eq!(text(b".str.1"), "@.str.1");
        assert_eq!(text(b"a-b$c"), "@a-b$c");
        assert_eq!(text(b"1abc"), "@\"1abc\"");
        assert_eq!(text(b"a b"), "@\"a b\"");
        assert_eq!(text(b"a\"b\\c"), "@\"a\\22b\\5Cc\"");
        assert_eq!(text(&[b'a', 0x01, 0xff, b'z']), "@\"a\\01\\FFz\"");
        assert_eq!(text(b"operator()"), "@\"operator()\"");
    }

    /// §9.1: the dialects are LLVM 19 to 22.
    #[test]
    fn dialects_are_19_to_22() {
        for major in 19..=22 {
            assert_eq!(Dialect::new(major).unwrap().major(), major);
        }
        for major in [0, 18, 23] {
            let error = Dialect::new(major).unwrap_err();
            assert!(matches!(error, PrintError::Unsupported { ref construct, .. }
                if *construct == format!("dialect:llvm-{major}")));
        }
        assert_eq!(Dialect::ALL.map(Dialect::major), [19, 20, 21, 22]);
    }

    /// §9.1: an `io::Write` takes the text, and its error comes back.
    #[test]
    fn io_sink_writes_and_reports() {
        struct Full;
        impl io::Write for Full {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("full"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut sink = IoSink::new(Vec::new());
        sink.write_str("abc").unwrap();
        assert_eq!(sink.finish().unwrap(), b"abc");

        let mut sink = IoSink::new(Full);
        assert!(sink.write_str("abc").is_err());
        assert_eq!(sink.finish().map(|_| ()).unwrap_err().to_string(), "full");
    }
}
