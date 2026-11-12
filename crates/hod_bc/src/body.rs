//! Function blocks (`spec/bc.md` §11): a FUNCTION block read into a
//! `hod_ir` body, making the decisions `hod_ll` makes for the same body
//! as `llvm-dis` text (`spec/ll.md` §11). Instructions are in `inst.rs`.
//!
//! A [`BodyReader`] borrows the [`ModuleBits`] of its module and keeps its
//! own copy of the module's constant memo, so any number of readers, each
//! with its own clone of the module, read bodies in parallel.

use std::borrow::Cow;
use std::collections::HashMap;

use hod_ir::{Body, FunctionBuilder, IrErrorKind, Module, ParamAttrs, TypeId, TypeKind, Value};

use crate::attrs::FnGroup;
use crate::bitstream::{Cursor, Entry};
use crate::codes::{block, func, md};
use crate::consts::{Recs, Values};
use crate::error::{BcError, BcErrorKind, BcResult, fail, ir};
use crate::module::{FunctionBits, ModuleBits};
use crate::raw::Rec;
use crate::types::note_names;

/// A value of the body: what it is, its type, and the notes its type
/// makes where it is used (`spec/bc.md` §11.3).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Local {
    pub value: Value,
    pub ty: TypeId,
    pub notes: u8,
}

/// What is known of a symbol as a callee (as `hod_ll`'s `CalleeInfo`).
#[derive(Clone, Copy, Debug)]
pub(crate) enum CalleeInfo {
    Unknown,
    NoCode(hod_ir::NoCode),
    Known(hod_ir::Intrinsic<'static>, [TypeId; 3], u8),
    Other,
    Plain,
}

/// Everything a reader keeps from one function to the next.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub storage: Option<Body>,
    pub ops: Vec<u64>,
    /// The function's CONSTANTS records and their operands.
    pub const_pool: Vec<u64>,
    pub const_recs: Vec<Rec>,
    /// The instruction values defined, by number from the first.
    pub insts: Vec<Local>,
    /// Forward references: instruction number, placeholder and type.
    pub forwards: HashMap<u32, (Value, TypeId)>,
    pub pending: Vec<Cow<'static, str>>,
    pub definition_notes: Vec<Cow<'static, str>>,
    pub param_notes: Vec<u8>,
    pub params: Vec<TypeId>,

    pub vals: Vec<Value>,
    pub arg_attrs: Vec<ParamAttrs>,
    pub bundle_vals: Vec<Value>,
    /// Per bundle: the tag (in `bundle_tags`) and the inputs (in
    /// `bundle_vals`).
    pub bundles: Vec<(u32, u32, u32, u32)>,
    pub bundle_tags: String,
    pub bundle_metadata: bool,
    pub incoming: Vec<(Value, hod_ir::BlockId)>,
    pub targets: Vec<hod_ir::BlockId>,
    pub cases: Vec<(hod_ir::ConstId, hod_ir::BlockId)>,
    pub case_ids: Vec<u32>,
    pub indices: Vec<(Value, u64)>,
    pub mask: Vec<u32>,
    pub path: Vec<u32>,
    pub clauses: Vec<hod_ir::Clause>,
    pub callees: Vec<CalleeInfo>,
    /// Call-site function groups: id, kept flags, names rejected.
    pub fn_groups: Vec<(u64, u32, Vec<String>)>,
    /// The call of a debug intrinsic waiting for its location.
    pub old_record: Option<crate::body_debug::OldRecord>,
    /// The values of the debug record being read.
    pub dbg_vals: Vec<Value>,
}

/// Reads the bodies of one module's functions (`spec/bc.md` §11).
///
/// A reader belongs to one [`ModuleBits`] and is used with the module
/// read with it, or with one clone of it, throughout: the constants it
/// builds are added to that module. After the largest function, reading
/// a body allocates little.
///
/// ```
/// # fn main() -> Result<(), hod_bc::BcError> {
/// # let bitcode: &[u8] = &[];
/// # if bitcode.is_empty() { return Ok(()); }
/// let mut read = hod_bc::read_module(bitcode, &hod_bc::ReadOptions::default())?;
/// let mut reader = hod_bc::BodyReader::new(&read.bits);
/// for function in read.bits.functions() {
///     if let Some(body) = reader.read(&mut read.module, function)? {
///         reader.recycle(body);
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct BodyReader<'b, 'a> {
    pub(crate) bits: &'b ModuleBits<'a>,
    pub(crate) values: Values,
    pub(crate) state: State,
    unsupported: Vec<Cow<'static, str>>,
}

impl<'b, 'a> BodyReader<'b, 'a> {
    /// A reader for the functions of `bits`.
    pub fn new(bits: &'b ModuleBits<'a>) -> BodyReader<'b, 'a> {
        BodyReader {
            bits,
            values: bits.values.clone(),
            state: State::default(),
            unsupported: Vec::new(),
        }
    }

    /// Reads the body of `function`, one of `bits.functions()`, into
    /// `module` (the module read with the bits, or the clone this reader
    /// is used with).
    ///
    /// `Ok(None)` says that the body has a construct `hod_ir` cannot
    /// represent: each is named in [`BodyReader::unsupported`] and
    /// recorded in `module` against the function's symbol. The notes of
    /// the definition's own metadata attachments (`global-metadata:`)
    /// are recorded in `module` as well, and do not stop the body.
    ///
    /// # Errors
    /// [`BcError`] at the first record that is not what LLVM writes, or
    /// that `hod_ir` refuses. Never a panic, for any input.
    pub fn read(&mut self, module: &mut Module, function: &FunctionBits) -> BcResult<Option<Body>> {
        self.unsupported.clear();
        let at = function.span.start;
        if function.sym.index() >= module.symbol_count() {
            return fail(BcErrorKind::Invalid, at, "this function is not a symbol of the module");
        }
        let result = self.body(module, function);
        let st = &mut self.state;
        if result.is_ok() {
            self.unsupported.append(&mut st.pending);
        }
        st.pending.clear();
        for construct in &self.unsupported {
            module.record_unsupported(construct, Some(function.sym));
        }
        for construct in st.definition_notes.drain(..) {
            module.record_unsupported(&construct, Some(function.sym));
        }
        result
    }

    /// The constructs that made the last body read unsupported, each
    /// once; empty for a body that was returned.
    pub fn unsupported(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.unsupported.iter().map(|construct| &**construct)
    }

    /// Gives the reader a body its consumer is done with: the next body
    /// is built in its storage.
    pub fn recycle(&mut self, body: Body) {
        self.state.storage = Some(body);
    }

    fn body(&mut self, module: &mut Module, function: &FunctionBits) -> BcResult<Option<Body>> {
        let at = function.span.start;
        let TypeKind::Function { ret, params, .. } = module.types.kind(function.ty) else {
            return fail(BcErrorKind::Type, at, "this function's type is not a function type");
        };
        let arg_count = params.len() as u64;
        let builder = match self.state.storage.take() {
            Some(storage) => FunctionBuilder::reusing(module, function.sym, function.ty, storage),
            None => FunctionBuilder::new(module, function.sym, function.ty),
        };
        let builder = ir(at, builder)?;
        let st = &mut self.state;
        st.insts.clear();
        st.forwards.clear();
        st.pending.clear();
        st.definition_notes.clear();
        st.bundles.clear();
        st.param_notes.clear();
        st.params.clear();
        st.params.extend_from_slice(params);
        st.old_record = None;
        if let Some(notes) = self.bits.types.param_notes(function.type_id) {
            st.param_notes.extend_from_slice(notes);
        }
        let base = self.bits.value_count() as u64;
        self.values.begin_function(base + arg_count, &[], &[])?;
        let mut fx = Fx {
            bits: self.bits,
            values: &mut self.values,
            module,
            b: builder,
            s: &mut self.state,
            ret_ty: ret,
            base,
            arg_count,
            local_end: base + arg_count,
            next: base + arg_count,
            declared: 0,
            begun: 0,
            open: false,
            at,
            room: (function.span.end - function.span.start) / 2,
            fdbg: self.bits.debug.function(function.span.start),
            inst_first: 0,
            last_loc: None,
        };
        let walked = fx.walk(function);
        let Fx { b, s, module, .. } = fx;
        if let Err(error) = walked {
            s.storage = Some(b.abandon());
            return Err(error);
        }
        if !s.pending.is_empty() {
            s.storage = Some(b.abandon());
            return Ok(None);
        }
        match b.finish() {
            // As `hod_ll`: a body whose debug records or locations break
            // a rule LLVM's verifier checks is `debug:invalid`.
            Ok(body) if !module.debug.is_empty() && body.verify_debug(module).is_err() => {
                s.pending.push(Cow::Borrowed(crate::debug::INVALID));
                s.storage = Some(body);
                Ok(None)
            }
            Ok(body) => Ok(Some(body)),
            // Every value has its definition, so what is left undefined
            // is a ring of no-code calls that each stand for the next.
            Err(error) if error.kind == IrErrorKind::UndefinedValue => {
                s.pending.push(Cow::Borrowed("no-code-cycle"));
                Ok(None)
            }
            Err(error) => Err(crate::error::ir_error(function.span.end, &error)),
        }
    }
}

/// One function's body as [`read_bodies`] hands it over.
#[derive(Debug)]
pub struct ReadBody<'p> {
    /// The function.
    pub function: &'p FunctionBits,
    /// The body; `None` for a function with an unsupported construct.
    pub body: Option<&'p Body>,
    /// The unsupported constructs of the body, each once.
    pub unsupported: &'p [Cow<'static, str>],
}

/// Reads the body of every function of `bits`, in the order of the
/// FUNCTION records, and hands each to `each` with the module as it then
/// is. One body is alive at a time and its storage is reused for the
/// next.
///
/// # Errors
/// The first [`BcError`] of a body, converted to `E`, or the first error
/// `each` returns.
pub fn read_bodies<E: From<BcError>>(
    module: &mut Module,
    bits: &ModuleBits<'_>,
    mut each: impl FnMut(&Module, ReadBody<'_>) -> Result<(), E>,
) -> Result<(), E> {
    let mut reader = BodyReader::new(bits);
    for function in bits.functions() {
        let body = reader.read(module, function)?;
        let read = ReadBody { function, body: body.as_ref(), unsupported: &reader.unsupported };
        each(module, read)?;
        if let Some(body) = body {
            reader.recycle(body);
        }
    }
    Ok(())
}

/// [`read_bodies`] on up to `threads` threads, the calling one included
/// (`spec/bc.md` §7): the module is `module(host)`, and each body goes to
/// `each` in the order of the FUNCTION records, translated to the
/// module's ids, with the module as reading the bodies before it and
/// that one sequentially leaves it. A body is `Ok` or the constructs that
/// made it unsupported. `each` returns whether to go on.
///
/// With one thread this is [`BodyReader::read`] of every function in
/// order. With more, the module's tables, every id and every body are
/// the same as with one (`spec/ir.md` §5.1).
///
/// # Errors
/// The [`BcError`] of the first body in file order that has one: the
/// error a sequential reading returns. `Ok`: how the reading went.
pub fn read_bodies_on<H>(
    host: &mut H,
    module: fn(&mut H) -> &mut Module,
    bits: &ModuleBits<'_>,
    threads: usize,
    mut each: impl FnMut(&mut H, &FunctionBits, Result<Body, &[Box<str>]>) -> bool,
) -> Result<hod_ir::ReadStats, BcError> {
    let functions = bits.functions();
    let new_reader = || BodyReader::new(bits);
    let mut deliver = |host: &mut H, index: usize, read: hod_ir::BodyRead<'_>| {
        let read = match read {
            hod_ir::BodyRead::Body(body) => Ok(body),
            hod_ir::BodyRead::Unsupported(constructs) => Err(constructs),
        };
        each(host, &functions[index], read)
    };
    hod_ir::read_bodies(host, module, functions.len(), threads, &new_reader, &mut deliver)
}

impl hod_ir::BodySource for BodyReader<'_, '_> {
    type Error = BcError;

    fn read_body(&mut self, module: &mut Module, index: usize) -> BcResult<Option<Body>> {
        let bits = self.bits;
        self.read(module, &bits.functions()[index])
    }

    fn unsupported(&self) -> Vec<Box<str>> {
        self.unsupported.iter().map(|construct| (**construct).into()).collect()
    }

    /// The body block's bytes, four times: bitcode takes about a quarter
    /// of the bytes of the same body as text.
    fn weight(&self, index: usize) -> usize {
        let span = &self.bits.functions()[index].span;
        ((span.end - span.start) / 2) as usize
    }
}

/// The reader of one function block.
pub(crate) struct Fx<'s, 'b, 'a> {
    pub bits: &'b ModuleBits<'a>,
    pub values: &'s mut Values,
    pub module: &'s mut Module,
    pub b: FunctionBuilder,
    pub s: &'s mut State,
    pub ret_ty: TypeId,
    /// The first value id of the function: the module's value count.
    pub base: u64,
    pub arg_count: u64,
    /// The value id of the first instruction.
    pub local_end: u64,
    /// The value id the next instruction with a result takes.
    pub next: u64,
    /// DECLAREBLOCKS, and how many blocks were begun.
    pub declared: u32,
    pub begun: u32,
    /// Whether a block is begun and not yet ended.
    pub open: bool,
    /// The position of the record being read.
    pub at: u64,
    /// The most blocks the function block has room for.
    pub room: u64,
    /// The function's local metadata.
    pub fdbg: crate::debug::FnDebug,
    /// The builder's instruction count before the last instruction read:
    /// its first instruction, where its location starts and its debug
    /// records stand.
    pub inst_first: usize,
    /// The location of the last `DEBUG_LOC`.
    pub last_loc: Option<hod_ir::LocId>,
}

/// The operands of one record, read in order.
#[allow(clippy::struct_field_names)] // `ops` are what the record holds.
pub(crate) struct Ops<'o> {
    pub ops: &'o [u64],
    pub i: usize,
    pub at: u64,
}

impl Ops<'_> {
    pub fn next(&mut self) -> BcResult<u64> {
        match self.ops.get(self.i) {
            Some(&op) => {
                self.i += 1;
                Ok(op)
            }
            None => fail(BcErrorKind::Invalid, self.at, "an instruction record cut short"),
        }
    }

    pub fn left(&self) -> usize {
        self.ops.len() - self.i
    }

    pub fn u32(&mut self) -> BcResult<u32> {
        let at = self.at;
        u32::try_from(self.next()?)
            .or_else(|_| fail(BcErrorKind::Invalid, at, "an operand over 32 bits"))
    }
}

impl Fx<'_, '_, '_> {
    /// Notes `construct` for the body (`spec/bc.md` §11.6), once.
    pub fn note(&mut self, construct: impl Into<Cow<'static, str>>) {
        let construct = construct.into();
        if !self.s.pending.contains(&construct) {
            self.s.pending.push(construct);
        }
    }

    pub fn note_bits(&mut self, bits: u8) {
        if bits != 0 {
            for name in note_names(bits) {
                self.note(name);
            }
        }
    }

    pub fn invalid<T>(&self, message: &str) -> BcResult<T> {
        fail(BcErrorKind::Invalid, self.at, message)
    }

    pub fn mistyped<T>(&self, message: &str) -> BcResult<T> {
        fail(BcErrorKind::Type, self.at, message)
    }

    /// Bitcode type `id`, noting what a use of it notes.
    pub fn ty(&mut self, id: u64) -> BcResult<TypeId> {
        let ty = self.bits.types.get(id, self.at)?;
        self.note_bits(self.bits.types.notes_of(id));
        Ok(ty)
    }

    // ---- the walk ----

    fn walk(&mut self, function: &FunctionBits) -> BcResult<()> {
        let bits = self.bits;
        let mut cursor = Cursor::enter_at(bits.data(), bits.block_info(), function.span)?;
        let mut ops = std::mem::take(&mut self.s.ops);
        let result = self.records(&mut cursor, &mut ops);
        self.s.ops = ops;
        result?;
        self.flush_old_record();
        if self.open || self.begun != self.declared {
            return fail(
                BcErrorKind::Invalid,
                function.span.end,
                "a function block whose blocks are not all ended",
            );
        }
        if !self.s.forwards.is_empty() {
            return fail(
                BcErrorKind::Invalid,
                function.span.end,
                "a use of a value the function does not define",
            );
        }
        Ok(())
    }

    fn records(&mut self, cursor: &mut Cursor<'_, '_>, ops: &mut Vec<u64>) -> BcResult<()> {
        let mut constants_read = false;
        loop {
            match cursor.next()? {
                Entry::End => return Ok(()),
                Entry::Block(id) => {
                    self.at = cursor.entry_pos();
                    match id {
                        block::CONSTANTS => {
                            if constants_read || self.next != self.local_end {
                                return self.invalid("a CONSTANTS block after the instructions");
                            }
                            constants_read = true;
                            cursor.enter()?;
                            self.constants(cursor, ops)?;
                        }
                        block::METADATA_ATTACHMENT => {
                            cursor.enter()?;
                            self.attachments(cursor, ops)?;
                        }
                        _ => cursor.skip(),
                    }
                }
                Entry::Record(abbrev) => {
                    self.at = cursor.entry_pos();
                    let (code, _) = cursor.record(abbrev, ops)?;
                    let mut o = Ops { ops, i: 0, at: self.at };
                    self.record(code, &mut o)?;
                }
            }
        }
    }

    /// The function's CONSTANTS block, into the function layer of the
    /// value table.
    fn constants(&mut self, cursor: &mut Cursor<'_, '_>, ops: &mut Vec<u64>) -> BcResult<()> {
        self.s.const_pool.clear();
        self.s.const_recs.clear();
        loop {
            match cursor.next()? {
                Entry::End => break,
                Entry::Block(_) => {
                    return fail(
                        BcErrorKind::Invalid,
                        cursor.entry_pos(),
                        "a block inside a CONSTANTS block",
                    );
                }
                Entry::Record(abbrev) => {
                    let pos = cursor.entry_pos();
                    let (code, blob) = cursor.record(abbrev, ops)?;
                    let start = self.s.const_pool.len();
                    match blob {
                        // A blob (as `DATA` never is): its bytes as
                        // characters, as the module level keeps them.
                        Some(bytes) => {
                            self.s.const_pool.extend(bytes.iter().map(|&b| u64::from(b)));
                        }
                        None => self.s.const_pool.extend_from_slice(ops),
                    }
                    let len = self.s.const_pool.len() - start;
                    let (Ok(start), Ok(len)) = (u32::try_from(start), u32::try_from(len)) else {
                        return fail(BcErrorKind::Limit, pos, "a CONSTANTS block over 2^32 words");
                    };
                    self.s.const_recs.push(Rec::new(code, pos, start, len));
                }
            }
        }
        let first = self.base + self.arg_count;
        self.values.begin_function(first, &self.s.const_recs, &self.s.const_pool)?;
        self.local_end = first + self.values.local_len() as u64;
        self.next = self.local_end;
        Ok(())
    }

    /// The `METADATA_ATTACHMENT` block: the definition's own attachments
    /// are noted as `hod_ll` notes `define … !kind !N`; an instruction's
    /// are stepped over.
    fn attachments(&mut self, cursor: &mut Cursor<'_, '_>, ops: &mut Vec<u64>) -> BcResult<()> {
        loop {
            match cursor.next()? {
                Entry::End => return Ok(()),
                Entry::Block(_) => cursor.skip(),
                Entry::Record(abbrev) => {
                    let pos = cursor.entry_pos();
                    let (code, _) = cursor.record(abbrev, ops)?;
                    if code != md::ATTACHMENT {
                        return fail(
                            BcErrorKind::Unsupported("record"),
                            pos,
                            format!("record code {code} in a METADATA_ATTACHMENT block"),
                        );
                    }
                    if !ops.len().is_multiple_of(2) {
                        continue;
                    }
                    for pair in ops.chunks(2) {
                        let kinds = self.bits.metadata_kinds();
                        let Some((_, name)) = kinds.iter().find(|(id, _)| *id == pair[0]) else {
                            return fail(
                                BcErrorKind::Invalid,
                                pos,
                                format!("metadata kind {} is not defined", pair[0]),
                            );
                        };
                        match &name[..] {
                            b"dbg" | b"type" | b"vcall_visibility" | b"prof"
                            | b"section_prefix" | b"annotation" => {}
                            _ => {
                                let construct: Cow<'static, str> = Cow::Owned(format!(
                                    "global-metadata:{}",
                                    String::from_utf8_lossy(name)
                                ));
                                if !self.s.definition_notes.contains(&construct) {
                                    self.s.definition_notes.push(construct);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// One record of the function block.
    fn record(&mut self, code: u32, o: &mut Ops<'_>) -> BcResult<()> {
        match code {
            func::DECLAREBLOCKS => {
                if self.declared != 0 || self.begun != 0 {
                    return self.invalid("a second DECLAREBLOCKS record");
                }
                let n = o.next()?;
                // Each block holds a record of at least a few bits.
                if n == 0 || n > self.room {
                    return self.invalid("a DECLAREBLOCKS count no function has");
                }
                for _ in 0..n {
                    ir(self.at, self.b.create_block())?;
                }
                self.declared = n as u32;
                Ok(())
            }
            func::DEBUG_LOC => self.debug_loc(o),
            func::DEBUG_LOC_AGAIN => self.debug_loc_again(),
            func::DEBUG_RECORD_VALUE..=func::DEBUG_RECORD_DECLARE_VALUE => {
                self.debug_record(code, o)
            }
            func::BLOCKADDR_USERS => Ok(()),
            func::OPERAND_BUNDLE => self.bundle(o),
            func::GEP_OLD
            | func::SELECT
            | func::CMP
            | func::STORE_OLD
            | func::INBOUNDS_GEP_OLD
            | func::CMPXCHG_OLD
            | func::ATOMICRMW_OLD
            | func::LANDINGPAD_OLD
            | func::STOREATOMIC_OLD => fail(
                BcErrorKind::Unsupported("record"),
                self.at,
                format!("instruction record code {code} of a bitcode before LLVM 19"),
            ),
            _ => {
                if !self.open {
                    if self.begun >= self.declared {
                        return self.invalid("an instruction after the last declared block");
                    }
                    let block = hod_ir::BlockId::new(self.begun);
                    ir(self.at, self.b.begin_block(block))?;
                    self.begun += 1;
                    self.open = true;
                }
                let is_call = matches!(code, func::CALL | func::INVOKE | func::CALLBR);
                if !is_call && !self.s.bundles.is_empty() {
                    return self.invalid("operand bundles before an instruction not a call");
                }
                self.flush_old_record();
                self.inst_first = self.b.inst_count();
                let (ty, value, terminator) = self.instruction(code, o)?;
                // The debug records after it stand before it.
                ir(self.at, self.b.place_records_before(Some(self.inst_first)))?;
                if terminator {
                    self.open = false;
                }
                if ty != TypeId::VOID {
                    self.define(ty, value)?;
                }
                Ok(())
            }
        }
    }

    // ---- values ----

    /// Gives the next instruction number its value.
    fn define(&mut self, ty: TypeId, value: Value) -> BcResult<()> {
        let index = self.s.insts.len();
        let Ok(number) = u32::try_from(index) else {
            return fail(BcErrorKind::Limit, self.at, "more than 2^32 instructions");
        };
        if let Some((placeholder, used)) = self.s.forwards.remove(&number) {
            if used != ty {
                return self.mistyped("an instruction referred to with another type than it has");
            }
            ir(self.at, self.b.resolve(placeholder, value))?;
        }
        self.s.insts.push(Local { value, ty, notes: 0 });
        self.next += 1;
        Ok(())
    }

    /// The value `id` (absolute), of type `ty` if one is expected; a
    /// forward reference needs one.
    pub fn get(&mut self, id: u64, ty: Option<TypeId>) -> BcResult<Local> {
        let local = if id < self.base {
            self.module_value(id)?
        } else if id < self.base + self.arg_count {
            let index = (id - self.base) as u32;
            let arg_ty = self.s.params[index as usize];
            let notes = self.s.param_notes.get(index as usize).copied().unwrap_or(0);
            Local { value: ir(self.at, self.b.arg(index))?, ty: arg_ty, notes }
        } else if id < self.local_end {
            if self.local_asm(id).is_some() {
                return self.mistyped("inline assembly used as a value");
            }
            self.constant(id)?
        } else {
            let Ok(number) = u32::try_from(id - self.local_end) else {
                return self.invalid("a value id past 2^32 instructions");
            };
            match self.s.insts.get(number as usize) {
                Some(&local) => local,
                None => {
                    if let Some(&(value, used)) = self.s.forwards.get(&number) {
                        Local { value, ty: used, notes: 0 }
                    } else {
                        let Some(ty) = ty else {
                            return self.invalid("a forward reference without its type");
                        };
                        if matches!(
                            self.module.types.kind(ty),
                            TypeKind::Void | TypeKind::Function { .. } | TypeKind::Label
                        ) {
                            return self.mistyped("a forward reference of a type no value has");
                        }
                        let value = ir(self.at, self.b.reserve(ty))?;
                        self.s.forwards.insert(number, (value, ty));
                        Local { value, ty, notes: 0 }
                    }
                }
            }
        };
        if let Some(ty) = ty
            && ty != local.ty
        {
            return self.mistyped("a value used with another type than it has");
        }
        self.note_bits(local.notes);
        Ok(local)
    }

    /// A module-level value: a global's address or a constant.
    fn module_value(&mut self, id: u64) -> BcResult<Local> {
        if let Some(global) = self.values.global(id) {
            let Some(sym) = global.sym else {
                return self.invalid("a use of one of the special arrays");
            };
            if global.ptr_ty == TypeId::PTR {
                let value = ir(self.at, Value::sym(sym))?;
                return Ok(Local { value, ty: TypeId::PTR, notes: 0 });
            }
        }
        self.constant(id)
    }

    /// Value `id`, a constant (or a global's address in another address
    /// space), as `hod_ll`'s `value` makes it.
    fn constant(&mut self, id: u64) -> BcResult<Local> {
        let raw = &self.bits.raw;
        let recs = Recs {
            module: (&raw.constants, &raw.pool),
            local: (&self.s.const_recs, &self.s.const_pool),
        };
        let (built, notes) = self.values.value(self.module, recs, id, self.at)?;
        let ty = self.module.consts.ty(built);
        // An expression that is just a symbol's address is the symbol.
        if ty == TypeId::PTR
            && let hod_ir::Const::Addr { sym, addend: 0 } = self.module.consts.get(built)
        {
            return Ok(Local { value: ir(self.at, Value::sym(sym))?, ty, notes });
        }
        Ok(Local { value: ir(self.at, Value::constant(built))?, ty, notes })
    }

    /// The record of the function's constant `id` if it is inline
    /// assembly.
    pub fn local_asm(&self, id: u64) -> Option<Rec> {
        let index = usize::try_from(id.checked_sub(self.base + self.arg_count)?).ok()?;
        let (_, _, rec) = self.values.local_slot(index)?;
        let rec = *self.s.const_recs.get(rec as usize)?;
        matches!(
            rec.code,
            crate::codes::cst::INLINEASM
                | crate::codes::cst::INLINEASM_OLD
                | crate::codes::cst::INLINEASM_OLD2
                | crate::codes::cst::INLINEASM_OLD3
        )
        .then_some(rec)
    }

    /// The relative value id `rel` as an absolute one.
    pub fn absolute(&self, rel: u64) -> u64 {
        u64::from((self.next as u32).wrapping_sub(rel as u32))
    }

    /// A value and, if it is a forward reference, its type.
    pub fn vt(&mut self, o: &mut Ops<'_>) -> BcResult<Local> {
        let id = self.absolute(o.next()?);
        if id < self.next {
            return self.get(id, None);
        }
        let ty = self.ty(o.next()?)?;
        self.get(id, Some(ty))
    }

    /// A value of type `ty`.
    pub fn v(&mut self, o: &mut Ops<'_>, ty: TypeId) -> BcResult<Local> {
        let id = self.absolute(o.next()?);
        self.get(id, Some(ty))
    }

    /// A value of type `ty` with a signed relative id (`phi`).
    pub fn v_signed(&mut self, o: &mut Ops<'_>, ty: TypeId) -> BcResult<Local> {
        let rel = crate::consts::signed(o.next()?);
        let id = u64::from((self.next as u32).wrapping_sub(rel as u32));
        self.get(id, Some(ty))
    }

    /// The block of absolute index `id`.
    pub fn block(&self, id: u64) -> BcResult<hod_ir::BlockId> {
        match u32::try_from(id) {
            Ok(id) if id < self.declared => Ok(hod_ir::BlockId::new(id)),
            _ => self.invalid("a block the function does not declare"),
        }
    }

    /// An `OPERAND_BUNDLE` record: a bundle of the next call.
    fn bundle(&mut self, o: &mut Ops<'_>) -> BcResult<()> {
        if self.s.bundles.is_empty() {
            self.s.bundle_vals.clear();
            self.s.bundle_tags.clear();
            self.s.bundle_metadata = false;
        }
        let tag = o.next()?;
        let Some(name) = usize::try_from(tag).ok().and_then(|i| self.bits.bundle_tags().get(i))
        else {
            return self.invalid("an operand bundle tag that is not defined");
        };
        let tag_start = self.s.bundle_tags.len() as u32;
        self.s.bundle_tags.push_str(&String::from_utf8_lossy(name));
        let tag_end = self.s.bundle_tags.len() as u32;
        let vals_start = self.s.bundle_vals.len() as u32;
        while o.left() > 0 {
            let input = self.vt(o)?;
            if input.ty == TypeId::METADATA {
                self.s.bundle_metadata = true;
                continue;
            }
            self.s.bundle_vals.push(input.value);
        }
        let vals_end = self.s.bundle_vals.len() as u32;
        self.s.bundles.push((tag_start, tag_end, vals_start, vals_end));
        Ok(())
    }

    /// The function-attribute group `id` of a call site: its kept flags
    /// and the names it rejects, built once.
    pub fn call_group(&mut self, id: u64, rec: Rec) -> BcResult<usize> {
        if let Some(found) = self.s.fn_groups.iter().position(|(gid, ..)| *gid == id) {
            return Ok(found);
        }
        let mut scratch = Vec::new();
        crate::attrs::decode(&rec.ops(&self.bits.raw.pool)[2..], rec.pos, &mut scratch)?;
        let FnGroup { attrs, rejected, .. } =
            crate::attrs::fn_group(self.module, &scratch, rec.pos)?;
        let flags = self.module.fn_attrs(attrs).flags();
        self.s.fn_groups.push((id, flags, rejected));
        Ok(self.s.fn_groups.len() - 1)
    }

    /// The group entry `id` of the attribute tables: its index and record.
    pub fn group(&self, id: u64) -> BcResult<(u64, Rec)> {
        let groups = &self.bits.groups;
        match groups.binary_search_by_key(&id, |&(gid, _, _)| gid) {
            Ok(found) => Ok((groups[found].1, groups[found].2)),
            Err(_) => self.invalid("an attribute group that is not defined"),
        }
    }
}
