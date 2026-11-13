//! Debug information as text (`spec/ir.md` §9.7): the nodes, the
//! locations and `!llvm.dbg.cu` at the end of the module; `!dbg` on
//! functions, variables and instructions; the debug records of a body.
//!
//! Numbering: node `DiId(i)` is `!i`, location `LocId(l)` is
//! `!(node_count + l)`, and the module flags and lists that follow start
//! after the last location. A module without debug information numbers
//! its metadata from 0, as before. Strings and constants used as nodes
//! are written in place where they are named, never numbered; lists are
//! written in place as `!{...}`, expressions as `!DIExpression(...)`.

use std::fmt::Write;

use super::{PrintError, R, Writer, write_quoted};
use crate::body::{Body, DbgDeclare, DbgRecord, DbgRecordKind, Inst};
use crate::debug::expr::{DI_OPS, ExprId};
use crate::debug::names::{
    DiFlags, SpFlags, dw_ate_name, dw_cc_name, dw_lang_name, dw_op_name, dw_tag_name,
};
use crate::debug::node::{DiBound, DiEnumerator, DiFieldOut, DiKind, DiNode};
use crate::ids::{DiId, LocId, SymId};
use crate::inst::Opcode;
use crate::symbol::{Symbol, SymbolKind};
use crate::value::{InstId, Value, ValueKind};

/// The fields LLVM's parser requires, and those whose default there is
/// not hod's (`spFlags`, `isDefinition`, `splitDebugInlining`): written even at hod's default.
const REQUIRED: &[(DiKind, &str)] = &[
    (DiKind::Subprogram, "spFlags"),
    (DiKind::GlobalVariable, "isDefinition"),
    // WP-062: LLVM's default is `true`, hod's `false`.
    (DiKind::CompileUnit, "splitDebugInlining"),
    (DiKind::File, "filename"),
    (DiKind::File, "directory"),
    (DiKind::Enumerator, "name"),
    (DiKind::Enumerator, "value"),
    (DiKind::DerivedType, "baseType"),
    (DiKind::SubroutineType, "types"),
    (DiKind::LexicalBlockFile, "discriminator"),
    (DiKind::Namespace, "scope"),
    (DiKind::TemplateTypeParameter, "type"),
    (DiKind::TemplateValueParameter, "value"),
    (DiKind::GlobalVariableExpression, "var"),
    (DiKind::GlobalVariableExpression, "expr"),
    (DiKind::ImportedEntity, "scope"),
    (DiKind::Label, "name"),
    (DiKind::Label, "file"),
    (DiKind::Label, "line"),
];

/// The fields LLVM's text has only from a major on.
const SINCE: &[(DiKind, &str, u32)] = &[
    (DiKind::Label, "column", 21),
    (DiKind::Label, "isArtificial", 21),
    (DiKind::Label, "coroSuspendIdx", 21),
    (DiKind::BasicType, "dataSize", 21),
];

/// The record fields of `kind` in the order LLVM's printer writes them
/// differ from hod's only in where `tag` stands; both orders parse.
impl<W: Write + ?Sized> Writer<'_, W> {
    /// The metadata number of a node.
    fn node_number(&self, id: DiId) -> Result<u32, PrintError> {
        if !self.m.debug.contains(id) {
            return self.malformed(format!("{id:?} is not a debug node of the module"));
        }
        Ok(id.raw())
    }

    /// The metadata number of a location.
    fn loc_number(&self, id: LocId) -> Result<u32, PrintError> {
        if id.index() >= self.m.debug.loc_count() {
            return self.malformed(format!("{id:?} is not a location of the module"));
        }
        Ok((self.m.debug.node_count() + id.index()) as u32)
    }

    /// The first metadata number after the debug nodes and locations.
    pub(super) fn debug_numbers(&self) -> u32 {
        let debug = &self.m.debug;
        (debug.node_count() + debug.loc_count()) as u32
    }

    /// A reference to a node where LLVM expects metadata: `!N`, or in
    /// place for a string or a constant; `null` for none.
    fn md_ref(&mut self, id: DiId) -> R {
        if id.is_none() {
            return self.s("null");
        }
        let number = self.node_number(id)?;
        match self.m.debug.node(id) {
            DiNode::String(string) => {
                self.s("!")?;
                write_quoted(self.out, string.text)
            }
            DiNode::Constant(constant) => self.typed_const(constant.value),
            DiNode::Other(other) => {
                let kind = String::from_utf8_lossy(other.kind);
                self.unsupported(format!("debug:{kind}"))
            }
            _ => Ok(write!(self.out, "!{number}")?),
        }
    }

    /// `!{a, b}` in place.
    fn md_list(&mut self, list: &[DiId]) -> R {
        self.s("!{")?;
        for (at, &id) in list.iter().enumerate() {
            if at > 0 {
                self.s(", ")?;
            }
            self.md_ref(id)?;
        }
        self.s("}")
    }

    /// `!DIExpression(...)`.
    pub(super) fn md_expr(&mut self, id: ExprId) -> R {
        if id.index() >= self.m.debug.expr_count() {
            return self.malformed(format!("{id:?} is not an expression of the module"));
        }
        let elements = self.m.debug.expr_elements(id);
        self.s("!DIExpression(")?;
        let mut at = 0;
        while at < elements.len() {
            if at > 0 {
                self.s(", ")?;
            }
            let code = elements[at];
            let Some(&(name, _, operands)) = DI_OPS.iter().find(|op| op.1 == code) else {
                let name = dw_op_name(code).unwrap_or_else(|| code.to_string());
                return self.unsupported(format!("debug:{name}"));
            };
            self.s(name)?;
            for operand in elements.iter().skip(at + 1).take(usize::from(operands)) {
                write!(self.out, ", {operand}")?;
            }
            at += 1 + usize::from(operands);
        }
        self.s(")")
    }

    /// A number field: by its DWARF name when the field holds a DWARF
    /// constant LLVM writes by name.
    fn md_number(&mut self, field: &str, value: u64) -> R {
        let short = u16::try_from(value).ok();
        let name = short.and_then(|value| match field {
            "tag" => dw_tag_name(value),
            "encoding" => dw_ate_name(value),
            "language" | "runtimeLang" if value != 0 => dw_lang_name(value),
            "cc" => dw_cc_name(value),
            _ => None,
        });
        match name {
            Some(name) => self.s(name),
            None => Ok(write!(self.out, "{value}")?),
        }
    }

    /// Flag bits as `A | B`; bits without a name as a number.
    fn md_flags(&mut self, bits: u32, names: &[(&str, u32)], fields: &[u32]) -> R {
        let mut rest = bits;
        let mut first = true;
        let mut put = |this: &mut Self, name: &str| -> R {
            if !first {
                this.s(" | ")?;
            }
            first = false;
            this.s(name)
        };
        // The multi-bit fields first: their values are not single bits.
        for &mask in fields {
            let value = bits & mask;
            if value != 0
                && let Some((name, _)) = names.iter().find(|(_, known)| *known == value)
            {
                put(self, name)?;
                rest &= !mask;
            }
        }
        for &(name, known) in names {
            if known.is_power_of_two()
                && fields.iter().all(|mask| mask & known == 0)
                && rest & known != 0
            {
                put(self, name)?;
                rest &= !known;
            }
        }
        if rest != 0 || bits == 0 {
            if !first {
                self.s(" | ")?;
            }
            write!(self.out, "{rest}")?;
        }
        Ok(())
    }

    fn md_bound(&mut self, bound: DiBound) -> R {
        match bound {
            DiBound::None => self.s("null"),
            DiBound::Const(value) => Ok(write!(self.out, "{value}")?),
            DiBound::Node(id) => self.md_ref(id),
            DiBound::Expr(id) => self.md_expr(id),
        }
    }

    /// One `name: value` of a node.
    fn md_field(&mut self, node: &DiNode<'_>, field: &str, value: DiFieldOut<'_>) -> R {
        write!(self.out, "{field}: ")?;
        match value {
            DiFieldOut::UInt(value) => self.md_number(field, value),
            DiFieldOut::Int(value) => match node {
                DiNode::Enumerator(DiEnumerator { is_unsigned: true, .. }) => {
                    Ok(write!(self.out, "{}", value as u64)?)
                }
                _ => Ok(write!(self.out, "{value}")?),
            },
            DiFieldOut::Bool(value) => Ok(write!(self.out, "{value}")?),
            DiFieldOut::Str(text) => write_quoted(self.out, text),
            DiFieldOut::Node(id) => self.md_ref(id),
            DiFieldOut::List(list) => self.md_list(list),
            DiFieldOut::Expr(id) => self.md_expr(id),
            DiFieldOut::Const(id) => self.typed_const(id),
            DiFieldOut::Bound(bound) => self.md_bound(bound),
            DiFieldOut::Flags(flags) => self.md_flags(flags.0, DiFlags::NAMES, &[3, 3 << 16]),
            DiFieldOut::SpFlags(flags) => self.md_flags(flags.0, SpFlags::NAMES, &[3]),
            DiFieldOut::EmissionKind(kind) => self.s(kind.name()),
            DiFieldOut::NameTableKind(kind) => self.s(kind.name()),
            DiFieldOut::ChecksumKind(kind) => self.s(kind.name()),
        }
    }

    /// The value a required field has at its default.
    fn md_default(&mut self, field: &str, kind: DiKind) -> R {
        write!(self.out, "{field}: ")?;
        match kind.field_shape(field.as_bytes()) {
            Some(crate::debug::node::DiShape::Str) => self.s("\"\""),
            Some(crate::debug::node::DiShape::Node | crate::debug::node::DiShape::List) => {
                self.s("null")
            }
            Some(crate::debug::node::DiShape::Expr) => self.s("!DIExpression()"),
            Some(crate::debug::node::DiShape::Bool) => self.s("false"),
            _ => self.s("0"),
        }
    }

    /// `!N = [distinct ]!DIKind(...)` for every node that is numbered.
    fn debug_nodes(&mut self) -> R {
        let m = self.m;
        let major = self.dialect.major();
        let mut fields: Vec<(&'static str, DiFieldOut<'_>)> = Vec::new();
        for index in 0..m.debug.node_count() {
            let id = DiId::new(index as u32);
            let node = m.debug.node(id);
            let kind = node.kind();
            match node {
                DiNode::String(_) | DiNode::Constant(_) => continue,
                DiNode::Undefined => return self.malformed(format!("{id:?} was never defined")),
                DiNode::Other(other) => {
                    let kind = String::from_utf8_lossy(other.kind);
                    return self.unsupported(format!("debug:{kind}"));
                }
                _ => {}
            }
            write!(self.out, "!{index} = ")?;
            if m.debug.is_distinct(id) {
                self.s("distinct ")?;
            }
            if let DiNode::Tuple(tuple) = node {
                self.md_list(tuple.elements)?;
                self.s("\n")?;
                continue;
            }
            fields.clear();
            node.each_field(&mut |field, value| fields.push((field, value)));
            write!(self.out, "!{}(", kind.llvm_name())?;
            let mut first = true;
            for &(field, value) in &fields {
                if let Some(&(_, _, since)) =
                    SINCE.iter().find(|(known, name, _)| *known == kind && *name == field)
                    && major < since
                {
                    let name = kind.llvm_name();
                    return self.unsupported(format!("debug:{name}.{field}"));
                }
                if !first {
                    self.s(", ")?;
                }
                first = false;
                self.md_field(&node, field, value)?;
            }
            for &(_, field) in REQUIRED.iter().filter(|(known, _)| *known == kind) {
                if fields.iter().all(|(present, _)| *present != field) {
                    if !first {
                        self.s(", ")?;
                    }
                    first = false;
                    self.md_default(field, kind)?;
                }
            }
            self.s(")\n")?;
        }
        Ok(())
    }

    /// `!N = [distinct ]!DILocation(...)` for every location.
    fn debug_locations(&mut self) -> R {
        let m = self.m;
        let base = m.debug.node_count();
        for index in 0..m.debug.loc_count() {
            let id = LocId::new(index as u32);
            let Some(loc) = m.debug.location(id) else {
                return self.malformed(format!("{id:?} was never defined"));
            };
            write!(self.out, "!{} = ", base + index)?;
            if m.debug.is_distinct_loc(id) {
                self.s("distinct ")?;
            }
            write!(self.out, "!DILocation(line: {}, column: {}, scope: ", loc.line, loc.column)?;
            self.md_ref(loc.scope)?;
            if loc.inlined_at != LocId::NONE {
                let number = self.loc_number(loc.inlined_at)?;
                write!(self.out, ", inlinedAt: !{number}")?;
            }
            // WP-149: LLVM writes each only when it is not 0.
            let atom = m.debug.atom(id);
            if atom.group != 0 {
                write!(self.out, ", atomGroup: {}", atom.group)?;
            }
            if atom.rank != 0 {
                write!(self.out, ", atomRank: {}", atom.rank)?;
            }
            self.s(")\n")?;
        }
        Ok(())
    }

    /// The debug nodes, the locations and `!llvm.dbg.cu`; nothing for a
    /// module without debug information.
    pub(super) fn debug_metadata(&mut self) -> R {
        let m = self.m;
        if m.debug.is_empty() {
            return Ok(());
        }
        self.debug_nodes()?;
        self.debug_locations()?;
        let units = m.debug.compile_units();
        if !units.is_empty() {
            self.s("!llvm.dbg.cu = !{")?;
            for (at, &unit) in units.iter().enumerate() {
                let number = self.node_number(unit)?;
                write!(self.out, "{}!{number}", if at > 0 { ", " } else { "" })?;
            }
            self.s("}\n")?;
        }
        Ok(())
    }

    // ---- attachments ----

    /// The subprogram a function's `!dbg` names, if the text may write it
    /// there: a definition's on a function printed with its body, a
    /// declaration's on one printed without.
    fn fn_subprogram(&self, symbol: Symbol<'_>, define: bool) -> Option<DiId> {
        let SymbolKind::Function { subprogram: Some(subprogram), .. } = symbol.kind() else {
            return None;
        };
        let distinct = self.m.debug.contains(subprogram) && self.m.debug.is_distinct(subprogram);
        (define || !distinct).then_some(subprogram)
    }

    /// ` !dbg !N` on a `define` line.
    pub(super) fn dbg_define(&mut self, symbol: Symbol<'_>) -> R {
        if let Some(subprogram) = self.fn_subprogram(symbol, true) {
            let number = self.node_number(subprogram)?;
            write!(self.out, " !dbg !{number}")?;
        }
        Ok(())
    }

    /// `!dbg !N ` after `declare`.
    pub(super) fn dbg_declare_fn(&mut self, symbol: Symbol<'_>) -> R {
        if let Some(subprogram) = self.fn_subprogram(symbol, false) {
            let number = self.node_number(subprogram)?;
            write!(self.out, "!dbg !{number} ")?;
        }
        Ok(())
    }

    /// `, !dbg !N` for each global-variable expression of a variable.
    pub(super) fn dbg_variable(&mut self, sym: SymId) -> R {
        let exprs = &self.global_dbg;
        let start = exprs.partition_point(|&(owner, _)| owner < sym);
        let end = start + exprs[start..].partition_point(|&(owner, _)| owner == sym);
        for at in start..end {
            let number = self.node_number(self.global_dbg[at].1)?;
            write!(self.out, ", !dbg !{number}")?;
        }
        Ok(())
    }

    /// `, !dbg !N` after an instruction that has a location.
    pub(super) fn dbg_inst(&mut self, inst: Inst<'_>) -> R {
        let loc = inst.loc();
        if loc != LocId::NONE {
            let number = self.loc_number(loc)?;
            write!(self.out, ", !dbg !{number}")?;
        }
        Ok(())
    }

    /// The tail of a record: `, !var, EXPR, !loc)`... as LLVM orders it.
    fn dbg_var_expr(&mut self, var: DiId, expr: ExprId) -> R {
        self.s(", ")?;
        self.md_ref(var)?;
        self.s(", ")?;
        self.md_expr(expr)
    }

    fn dbg_loc_close(&mut self, loc: LocId) -> R {
        let number = self.loc_number(loc)?;
        writeln!(self.out, ", !{number})")?;
        Ok(())
    }

    /// A value of a record: `type value`, or `!{}` for none.
    fn dbg_value(&mut self, body: &Body, value: Value) -> R {
        if value.is_none() {
            return self.s("!{}");
        }
        self.typed(body, value)
    }

    /// The whole-function declarations: `#dbg_declare` records before the
    /// first instruction of the entry block.
    pub(super) fn dbg_declares(&mut self, body: &Body) -> R {
        for var in body.slot_vars() {
            write!(self.out, "    #dbg_declare(ptr %s{}", var.slot.raw())?;
            self.dbg_var_expr(var.var, var.expr)?;
            self.dbg_loc_close(var.loc)?;
        }
        for declare in body.declares() {
            if placed_after(body, declare.address).is_none() {
                self.dbg_declare_one(body, declare)?;
            }
        }
        Ok(())
    }

    /// One `#dbg_declare` of [`Body::declares`].
    pub(super) fn dbg_declare_one(&mut self, body: &Body, declare: &DbgDeclare) -> R {
        self.s("    #dbg_declare(")?;
        self.dbg_value(body, declare.address)?;
        self.dbg_var_expr(declare.var, declare.expr)?;
        self.dbg_loc_close(declare.loc)
    }

    /// One positioned record.
    pub(super) fn dbg_record(&mut self, body: &Body, record: &DbgRecord<'_>) -> R {
        match record.kind {
            DbgRecordKind::Label => {
                self.s("    #dbg_label(")?;
                self.md_ref(record.var)?;
                self.dbg_loc_close(record.loc)
            }
            // The `DIAssignID` that ties the record to its stores is not
            // kept (§6.6): the text could not say the same.
            DbgRecordKind::Assign => self.unsupported("debug:#dbg_assign"),
            DbgRecordKind::Value => {
                self.s("    #dbg_value(")?;
                if record.arg_list {
                    self.s("!DIArgList(")?;
                    for (at, &value) in record.values.iter().enumerate() {
                        if at > 0 {
                            self.s(", ")?;
                        }
                        self.dbg_value(body, value)?;
                    }
                    self.s(")")?;
                } else {
                    let value = record.values.first().copied().unwrap_or(Value::NONE);
                    self.dbg_value(body, value)?;
                }
                self.dbg_var_expr(record.var, record.expr)?;
                self.dbg_loc_close(record.loc)
            }
        }
    }
}

/// The instruction after which a declaration of `address` is written:
/// the one that computes the address, unless that ends its block or is
/// a `phi` (then the declaration goes with those of the slots, at the
/// entry, where LLVM's text accepts the forward reference).
pub(super) fn placed_after(body: &Body, address: Value) -> Option<InstId> {
    let ValueKind::Inst(id) = address.kind() else {
        return None;
    };
    let inst = body.inst(id);
    (!inst.is_terminator() && inst.opcode() != Opcode::Phi).then_some(id)
}
