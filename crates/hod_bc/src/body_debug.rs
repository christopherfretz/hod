//! The debug information of a function block (`spec/bc.md` §11.9): the
//! instructions' locations (`DEBUG_LOC`, `DEBUG_LOC_AGAIN`), the debug
//! records that follow the instruction they stand before, and the calls
//! of the `llvm.dbg.*` intrinsics of the older form, read as `hod_ll`
//! reads the `llvm-dis` text of the same function.

use std::borrow::Cow;

use hod_ir::{DiAtom, DiId, DiKind, DiLocation, ExprId, Layering, LocId, Value, kit};

use crate::body::{Fx, Ops};
use crate::codes::func;
use crate::debug::{Handle, INVALID, LocalMd, column};
use crate::error::{BcResult, fail, ir};
use crate::module::Metadata;

/// A debug record, or a call of an `llvm.dbg.*` intrinsic read and
/// waiting for the call's `DEBUG_LOC`, which is the record's location;
/// its values are in `State::dbg_vals`.
pub(crate) use hod_ir::kit::DebugRecord as OldRecord;
/// The four debug records.
pub(crate) use hod_ir::kit::DebugRecordKind as RecordKind;

/// What a metadata id a body names is.
#[derive(Clone, Copy, Debug)]
enum Use {
    Debug(Handle),
    /// A value as metadata: bitcode type id and absolute value id.
    Value {
        ty: u64,
        value: u64,
    },
    EmptyTuple,
    /// `DebugBits::args[start..start + len]`.
    ArgList {
        start: u32,
        len: u32,
    },
    Other,
}

impl<L: Layering> Fx<'_, '_, '_, L> {
    /// What metadata `id` is: of the module, or of this function.
    fn md_use(&self, id: u64) -> BcResult<Use> {
        let debug = &self.bits.debug;
        let count = self.bits.metadata_count() as u64;
        if id < count {
            return Ok(match self.bits.metadata(id) {
                Some(Metadata::Value { ty, value }) => Use::Value { ty, value },
                Some(Metadata::Node { operands: [], .. }) => Use::EmptyTuple,
                _ => match debug.module.get(id as usize) {
                    Some(&handle) if handle != Handle::None => Use::Debug(handle),
                    _ => Use::Other,
                },
            });
        }
        let local = id - count;
        if local >= u64::from(self.fdbg.len) {
            return self.invalid("a metadata id the function does not define");
        }
        Ok(match debug.local[(u64::from(self.fdbg.start) + local) as usize] {
            LocalMd::Debug(handle) => Use::Debug(handle),
            LocalMd::Value { ty, value } => Use::Value { ty, value },
            LocalMd::EmptyTuple => Use::EmptyTuple,
            LocalMd::ArgList { start, len } => Use::ArgList { start, len },
            LocalMd::String { .. } | LocalMd::Node { .. } | LocalMd::Other => Use::Other,
        })
    }

    /// Value `value` of bitcode type `ty`, as a record's operand.
    fn md_value(&mut self, ty: u64, value: u64) -> BcResult<Value> {
        let ty = self.ty(ty)?;
        if ty == hod_ir::TypeId::METADATA {
            return self.invalid("a value as metadata of type metadata");
        }
        Ok(self.get(value, Some(ty))?.value)
    }

    /// The location a `DEBUG_LOC` record or a debug record names.
    fn location(
        &mut self,
        scope: u64,
        inlined_at: u64,
        line: u64,
        col: u64,
        atom: DiAtom,
    ) -> BcResult<LocId> {
        let Use::Debug(Handle::Node(scope)) = self.md_use(scope)? else {
            self.note(INVALID);
            return Ok(LocId::NONE);
        };
        let inlined_at = match inlined_at.checked_sub(1) {
            None => LocId::NONE,
            Some(id) => {
                let Use::Debug(Handle::Loc(loc)) = self.md_use(id)? else {
                    self.note(INVALID);
                    return Ok(LocId::NONE);
                };
                loc
            }
        };
        let Ok(line) = u32::try_from(line) else {
            return self.invalid("a DEBUG_LOC line over 32 bits");
        };
        let loc = DiLocation { line, column: column(col), scope, inlined_at };
        ir(self.at, self.module.debug.loc_in_atom(loc, atom))
    }

    /// `DEBUG_LOC`: `[line, column, scope + 1, inlinedAt + 1, implicit]`,
    /// and in LLVM 21 and later `atomGroup, atomRank` (WP-149, the
    /// location's atom).
    pub(crate) fn debug_loc(&mut self, o: &Ops<'_>) -> BcResult<()> {
        let &[line, col, scope, inlined_at, ..] = o.ops else {
            return self.invalid("a DEBUG_LOC record cut short");
        };
        if o.ops.len() > 7 {
            return fail(crate::debug::LAYOUT, self.at, "a DEBUG_LOC record of over 7 operands");
        }
        let Some(scope) = scope.checked_sub(1) else {
            return self.invalid("a DEBUG_LOC record without its scope");
        };
        let group = o.ops.get(5).copied().unwrap_or(0);
        let Ok(rank) = u8::try_from(o.ops.get(6).copied().unwrap_or(0)) else {
            return self.invalid("a DEBUG_LOC atom rank over 8 bits");
        };
        let loc = self.location(scope, inlined_at, line, col, DiAtom { group, rank })?;
        self.last_loc = Some(loc);
        self.loc_last(loc)
    }

    /// `DEBUG_LOC_AGAIN`: the location of the instruction before.
    pub(crate) fn debug_loc_again(&mut self) -> BcResult<()> {
        let Some(loc) = self.last_loc else {
            return self.invalid("a DEBUG_LOC_AGAIN record without a location before it");
        };
        self.loc_last(loc)
    }

    /// The location of the last instruction read, and the record of the
    /// debug intrinsic it called.
    fn loc_last(&mut self, loc: LocId) -> BcResult<()> {
        if loc != LocId::NONE {
            self.b.set_loc_since(self.inst_first, loc);
        }
        if let Some(record) = self.s.old_record.take() {
            self.old_record(record, loc)?;
        }
        Ok(())
    }

    /// The record of a debug intrinsic called without a location: LLVM's
    /// verifier wants one.
    pub(crate) fn flush_old_record(&mut self) {
        if self.s.old_record.take().is_some() {
            self.note(INVALID);
        }
    }

    fn old_record(&mut self, record: OldRecord, loc: LocId) -> BcResult<()> {
        if loc == LocId::NONE {
            self.note(INVALID);
            return Ok(());
        }
        self.emit_record(record, loc)
    }

    /// A debug record (codes 61 to 66), after the instruction it stands
    /// before.
    pub(crate) fn debug_record(&mut self, code: u32, o: &mut Ops<'_>) -> BcResult<()> {
        if code == func::DEBUG_RECORD_DECLARE_VALUE {
            let construct = kit::unsupported_record_construct(b"#dbg_declare_value");
            self.note(Cow::Borrowed(construct.unwrap_or("debug:#dbg_declare_value")));
            return Ok(());
        }
        let (kind, len) = match code {
            func::DEBUG_RECORD_VALUE | func::DEBUG_RECORD_VALUE_SIMPLE => (RecordKind::Value, 4),
            func::DEBUG_RECORD_DECLARE => (RecordKind::Declare, 4),
            func::DEBUG_RECORD_ASSIGN => (RecordKind::Assign, 7),
            _ => (RecordKind::Label, 2),
        };
        if o.ops.len() < len {
            return self.invalid("a debug record cut short");
        }
        if o.ops.len() > len && code != func::DEBUG_RECORD_VALUE_SIMPLE {
            return fail(crate::debug::LAYOUT, self.at, "a debug record of more operands");
        }
        let ops = o.ops;
        let record = if code == func::DEBUG_RECORD_VALUE_SIMPLE {
            o.i = 3;
            let value = self.vt(o)?.value;
            if o.left() != 0 {
                return fail(crate::debug::LAYOUT, self.at, "a debug record of more operands");
            }
            let record = self.record_args(kind, None, &ops[1..3])?;
            self.s.dbg_vals.push(value);
            record
        } else if kind == RecordKind::Label {
            self.record_args(kind, None, &ops[1..2])?
        } else {
            // An assignment: `[…, value, id, address_expr, address]`.
            let mds = [
                ops[1],
                ops[2],
                ops.get(4).copied().unwrap_or(0),
                *ops.get(6).unwrap_or(&0),
                *ops.get(5).unwrap_or(&0),
            ];
            self.record_args(kind, Some(ops[3]), &mds[..len - 2])?
        };
        let Use::Debug(Handle::Loc(loc)) = self.md_use(ops[0])? else {
            self.note(INVALID);
            return Ok(());
        };
        self.emit_record(record, loc)
    }

    /// A call of `llvm.dbg.*`: its metadata arguments (metadata ids),
    /// kept until the call's location is read.
    pub(crate) fn debug_call(&mut self, name: &[u8], args: &[u64]) -> BcResult<()> {
        let Some(kind) = RecordKind::of_intrinsic(name) else {
            self.note(kit::debug_intrinsic_construct(name));
            return Ok(());
        };
        let want = match kind {
            RecordKind::Label => 1,
            RecordKind::Assign => 6,
            _ => 3,
        };
        if args.len() != want {
            return self.invalid("a call of a debug intrinsic with the wrong arguments");
        }
        let record = match kind {
            RecordKind::Label => self.record_args(kind, None, args)?,
            _ => self.record_args(kind, Some(args[0]), &args[1..])?,
        };
        self.s.old_record = Some(record);
        Ok(())
    }

    /// A record's operands after its value: `var, expr` (`label` for a
    /// label), and for an assignment `id, address, address_expr`. The
    /// value (metadata id), if any, is pushed to `dbg_vals`.
    fn record_args(
        &mut self,
        kind: RecordKind,
        value: Option<u64>,
        mds: &[u64],
    ) -> BcResult<OldRecord> {
        let mut record = OldRecord::new(kind);
        self.s.dbg_vals.clear();
        if kind == RecordKind::Label {
            record.var = self.record_node(mds[0], kind.variable_kind())?;
            return Ok(record);
        }
        if let Some(value) = value {
            record.list = self.record_value(value)?;
        }
        record.var = self.record_node(mds[0], kind.variable_kind())?;
        record.expr = self.record_expr(mds[1])?;
        if kind == RecordKind::Assign {
            match self.md_use(mds[2])? {
                Use::Debug(Handle::AssignId) => {}
                _ => return self.invalid("expected a DIAssignID"),
            }
            let mark = self.s.dbg_vals.len();
            if self.record_value(mds[3])? {
                self.note(kit::ASSIGN_ARG_LIST);
            }
            record.address = self.s.dbg_vals.get(mark).copied().unwrap_or(Value::NONE);
            self.s.dbg_vals.truncate(mark);
            record.address_expr = self.record_expr(mds[4])?;
            if record.list {
                self.note(kit::ASSIGN_ARG_LIST);
            }
        }
        Ok(record)
    }

    /// A record's value: a value, `!{}` (none) or a `DIArgList` (true).
    /// Pushed to `dbg_vals`.
    fn record_value(&mut self, id: u64) -> BcResult<bool> {
        match self.md_use(id)? {
            Use::Value { ty, value } => {
                let value = self.md_value(ty, value)?;
                self.s.dbg_vals.push(value);
                Ok(false)
            }
            Use::EmptyTuple => {
                self.s.dbg_vals.push(Value::NONE);
                Ok(false)
            }
            Use::ArgList { start, len } => {
                let bits = self.bits;
                for at in start..start + len {
                    let arg = bits.debug.args[at as usize];
                    let Use::Value { ty, value } = self.md_use(arg)? else {
                        return self.invalid("a DIArgList operand that is not a value");
                    };
                    let value = self.md_value(ty, value)?;
                    self.s.dbg_vals.push(value);
                }
                Ok(true)
            }
            Use::Debug(_) | Use::Other => self.invalid("expected a value of a debug record"),
        }
    }

    /// The variable or label of a record: a node of `kind`.
    fn record_node(&mut self, id: u64, kind: DiKind) -> BcResult<DiId> {
        match self.md_use(id)? {
            Use::Debug(Handle::Node(node)) if self.module.debug.find_kind(node) == Some(kind) => {
                Ok(node)
            }
            _ => {
                self.note(INVALID);
                Ok(DiId::NONE)
            }
        }
    }

    /// The expression of a record.
    fn record_expr(&mut self, id: u64) -> BcResult<ExprId> {
        match self.md_use(id)? {
            Use::Debug(Handle::Expr(expr)) => Ok(expr),
            _ => self.invalid("expected a DIExpression"),
        }
    }

    /// Hands a record to the builder.
    fn emit_record(&mut self, record: OldRecord, loc: LocId) -> BcResult<()> {
        if !ir(self.at, kit::emit_debug_record(&mut self.b, &record, &self.s.dbg_vals, loc))? {
            self.note(INVALID);
        }
        Ok(())
    }
}
