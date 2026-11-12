//! Template parameters (`spec/dwarf.md` §6.11, WP-059), as `llc`'s
//! `addTemplateParams` writes them: `DW_TAG_template_type_parameter`
//! (`type`, `name`, `default_value`), `DW_TAG_template_value_parameter`
//! (`type`, `name`, `default_value`, then an integer as
//! `DW_AT_const_value`, `DW_FORM_udata` or `DW_FORM_sdata` by
//! `isUnsignedDIType`, or a symbol's address as `DW_AT_location
//! (DW_OP_addrx, DW_OP_stack_value)`), `DW_TAG_GNU_template_parameter_pack`
//! (`name`, the pack's parameters as children) and
//! `DW_TAG_GNU_template_template_param` (`name`, `default_value`,
//! `DW_AT_GNU_template_name`).
//!
//! A parameter is a child of its class, member function declaration,
//! template alias, variable or definition; each is a task of the type
//! graph, so that its DIE and then its type's are made in `llc`'s order.
//! `llc` writes nothing for a value it has no form for here (a float, a
//! null pointer, an address with an offset), and neither does hod.

use hod_ir::{Const, DiId, DiNode};
use hod_obj::SymbolId;

use crate::Error;
use crate::abbrev::{code, fam, tag};
use crate::die::{Kind, UnitPlan, Writer, bad, uleb_len, unsupported};
use crate::leb::{write_sleb, write_uleb};
use crate::scope::{TEMPLATE_ADDRESS, TEMPLATE_PARAMETER, TEMPLATE_VALUE};
use crate::types::{Task, Want};

const DW_OP_ADDRX: u8 = 0xa1;
const DW_OP_STACK_VALUE: u8 = 0x9f;

/// What a template value parameter's value is written as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Value {
    None,
    Int(u64, bool),
    Addr(hod_ir::SymId),
}

impl UnitPlan<'_> {
    /// Pushes the tasks of `params` as children of DIE `owner`, so that
    /// they run before what is on `work` (in order).
    pub(crate) fn template_tasks(owner: u32, params: &[DiId], work: &mut Vec<Task>) {
        for &p in params.iter().rev() {
            if p != DiId::NONE {
                work.push(Task::Template { parent: owner, node: p });
            }
        }
    }

    /// The parameters of a template alias (its `extraData` tuple).
    pub(crate) fn alias_template_tasks(
        &self,
        owner: u32,
        extra: Option<DiId>,
        work: &mut Vec<Task>,
    ) -> Result<(), Error> {
        let Some(t) = extra else { return Ok(()) };
        let DiNode::Tuple(tuple) = self.node_of(self.dies[owner as usize].node, "extraData", t)?
        else {
            return Err(bad(self.dies[owner as usize].node, "extraData"));
        };
        Self::template_tasks(owner, tuple.elements, work);
        Ok(())
    }

    /// `params` as children of DIE `owner`, now (a variable's, a
    /// definition's).
    pub(crate) fn add_template_params(&mut self, owner: u32, params: &[DiId]) -> Result<(), Error> {
        if params.is_empty() {
            return Ok(());
        }
        let mut work = std::mem::take(&mut self.build.work);
        work.clear();
        Self::template_tasks(owner, params, &mut work);
        let result = self.drain(&mut work);
        self.build.work = work;
        result
    }

    /// The value of template value parameter `p`.
    fn template_value(&self, p: DiId, value: Option<DiId>) -> Result<Value, Error> {
        let Some(v) = value else { return Ok(Value::None) };
        let DiNode::Constant(k) = self.node_of(p, "value", v)? else {
            return Err(unsupported(p, TEMPLATE_PARAMETER));
        };
        let Some((consts, types)) = self.consts else {
            return Err(unsupported(
                p,
                "a template parameter's value without the module's constants (UnitPlan::of_module)",
            ));
        };
        Ok(match consts.get(k.value) {
            Const::Int { ty, value } => {
                let DiNode::TemplateValueParameter(tp) = self.debug.node(p) else {
                    return Err(bad(p, "value"));
                };
                let unsigned = match tp.ty {
                    Some(t) => self.unsigned_di_type(t)?,
                    None => false,
                };
                let bits = types.int_bits(ty).unwrap_or(64).clamp(1, 64);
                let shift = 64 - bits;
                if unsigned {
                    Value::Int((value << shift) >> shift, false)
                } else {
                    Value::Int((((value << shift) as i64) >> shift) as u64, true)
                }
            }
            Const::WideInt { .. } => return Err(unsupported(p, TEMPLATE_VALUE)),
            Const::Addr { sym, addend: 0 } => Value::Addr(sym),
            _ => Value::None,
        })
    }

    /// `isUnsignedDIType` as `llc` 19 and 22 have it: an enumeration
    /// with a base type is its base type's signedness (§6.11).
    fn unsigned_di_type(&self, mut t: DiId) -> Result<bool, Error> {
        for _ in 0..=self.slot.len() {
            match self.node_of(t, "type", t)? {
                DiNode::CompositeType(c) if c.tag == tag::ENUMERATION_TYPE => match c.base_type {
                    Some(b) => t = b,
                    None => return Ok(false),
                },
                DiNode::DerivedType(d)
                    if !matches!(
                        d.tag,
                        tag::POINTER_TYPE
                            | tag::PTR_TO_MEMBER_TYPE
                            | tag::REFERENCE_TYPE
                            | tag::RVALUE_REFERENCE_TYPE
                    ) =>
                {
                    t = d.base_type.ok_or(bad(t, "baseType"))?;
                }
                _ => return self.is_unsigned(t),
            }
        }
        Err(Error::TypeCycle { node: t.raw() })
    }

    /// The task of one template parameter: its DIE under `parent`, then
    /// its type's (or a pack's parameters).
    pub(crate) fn template_task(
        &mut self,
        parent: u32,
        p: DiId,
        work: &mut Vec<Task>,
    ) -> Result<(), Error> {
        let owner = self.dies[parent as usize].node;
        let ensure = |t: DiId| Task::Ensure { from: p, field: "type", node: t, want: Want::Type };
        match self.node_of(owner, "templateParams", p)? {
            DiNode::TemplateTypeParameter(tp) => {
                self.push_anon(p, parent, Kind::TemplateParam);
                if let Some(t) = tp.ty {
                    work.push(ensure(t));
                }
            }
            DiNode::TemplateValueParameter(tp) => match tp.tag {
                tag::TEMPLATE_VALUE_PARAMETER => {
                    if let Value::Addr(sym) = self.template_value(p, tp.value)? {
                        self.scope_dies.want_symbol(sym);
                        self.scope_dies.param_addrs += 1;
                    }
                    self.push_anon(p, parent, Kind::TemplateParam);
                    if let Some(t) = tp.ty {
                        work.push(ensure(t));
                    }
                }
                tag::GNU_TEMPLATE_PARAMETER_PACK => {
                    if tp.defaulted || tp.ty.is_some() {
                        return Err(unsupported(p, TEMPLATE_PARAMETER));
                    }
                    let die = self.push_anon(p, parent, Kind::TemplateParam);
                    match tp.value.map(|v| self.node_of(p, "value", v)).transpose()? {
                        None => {}
                        Some(DiNode::Tuple(t)) => Self::template_tasks(die, t.elements, work),
                        Some(_) => return Err(unsupported(p, TEMPLATE_PARAMETER)),
                    }
                }
                tag::GNU_TEMPLATE_TEMPLATE_PARAM => {
                    match tp.value.map(|v| self.node_of(p, "value", v)).transpose()? {
                        Some(DiNode::String(_)) if tp.ty.is_none() => {}
                        _ => return Err(unsupported(p, TEMPLATE_PARAMETER)),
                    }
                    self.push_anon(p, parent, Kind::TemplateParam);
                }
                _ => return Err(unsupported(p, TEMPLATE_PARAMETER)),
            },
            _ => return Err(bad(owner, "templateParams")),
        }
        Ok(())
    }

    /// Gives the symbols template parameters take the address of their
    /// places, after the code generator placed them (§6.11): the symbol
    /// the `.debug_addr` entry is relocated against and the offset from
    /// it, or `None` for one this object does not define. Called after
    /// [`UnitPlan::seal`], before the unit is written; the bridge calls it
    /// with the same placement as the globals'.
    pub fn place_symbols(
        &mut self,
        mut place: impl FnMut(hod_ir::SymId) -> Option<(SymbolId, i64)>,
    ) {
        for s in &mut self.scope_dies.syms {
            s.1 = place(s.0);
        }
    }

    /// The `.debug_addr` entries the template parameters take.
    pub(crate) fn template_addrs(&self) -> usize {
        self.scope_dies.param_addrs as usize
    }
}

impl<'a> Writer<'a> {
    /// Writes a template parameter (§6.11).
    pub(crate) fn template_param_die(
        &mut self,
        plan: &UnitPlan<'a>,
        node: DiId,
        children: bool,
    ) -> Result<(), Error> {
        let b = u32::from;
        match plan.debug.node(node) {
            DiNode::TemplateTypeParameter(tp) => {
                let mask = b(tp.ty.is_some()) | b(!tp.name.is_empty()) << 1 | b(tp.defaulted) << 2;
                self.abbrev(code(fam::TEMPLATE_TYPE_PARAMETER, mask, false));
                if let Some(t) = tp.ty {
                    self.ref_to(plan, node, t)?;
                }
                self.opt_name(tp.name)?;
            }
            DiNode::TemplateValueParameter(tp) if tp.tag == tag::TEMPLATE_VALUE_PARAMETER => {
                let value = plan.template_value(node, tp.value)?;
                let ty = tp.ty.is_some();
                let name = !tp.name.is_empty();
                let place = match value {
                    Value::Addr(sym) => Some(
                        plan.scope_dies.placed(sym).ok_or(unsupported(node, TEMPLATE_ADDRESS))?,
                    ),
                    _ => None,
                };
                match value {
                    Value::Int(_, true) | Value::None => {
                        let mask = b(ty)
                            | b(name) << 1
                            | b(tp.defaulted) << 2
                            | b(value != Value::None) << 3;
                        self.abbrev(code(fam::TEMPLATE_VALUE_PARAMETER, mask, false));
                    }
                    Value::Addr(_) if !tp.defaulted => {
                        self.abbrev(code(fam::TEMPLATE_VALUE_ADDRESS, b(ty) | b(name) << 1, false));
                    }
                    Value::Int(_, false) | Value::Addr(_) => {
                        let mask = b(ty)
                            | b(name) << 1
                            | b(tp.defaulted) << 2
                            | b(matches!(value, Value::Int(..))) << 3
                            | b(matches!(value, Value::Addr(_))) << 5;
                        self.abbrev(code(fam::TEMPLATE_VALUE_ANY, mask, false));
                    }
                }
                if let Some(t) = tp.ty {
                    self.ref_to(plan, node, t)?;
                }
                self.opt_name(tp.name)?;
                match value {
                    Value::Int(bits, false) => write_uleb(&mut self.info, bits),
                    Value::Int(bits, true) => write_sleb(&mut self.info, bits as i64),
                    Value::Addr(_) => {
                        let (symbol, addend) = place.unwrap_or((SymbolId::from_index(0), 0));
                        let index = self.push_addr(symbol, addend);
                        write_uleb(&mut self.info, 2 + uleb_len(u64::from(index)) as u64);
                        self.info.push(DW_OP_ADDRX);
                        write_uleb(&mut self.info, u64::from(index));
                        self.info.push(DW_OP_STACK_VALUE);
                    }
                    Value::None => {}
                }
            }
            DiNode::TemplateValueParameter(tp) if tp.tag == tag::GNU_TEMPLATE_PARAMETER_PACK => {
                self.abbrev(code(fam::TEMPLATE_PACK, b(!tp.name.is_empty()), children));
                self.opt_name(tp.name)?;
            }
            DiNode::TemplateValueParameter(tp) => {
                let Some(DiNode::String(s)) = tp.value.map(|v| plan.debug.node(v)) else {
                    return Err(bad(node, "value"));
                };
                let name = !tp.name.is_empty();
                if tp.defaulted {
                    self.abbrev(code(fam::TEMPLATE_TEMPLATE_ANY, b(name) | 1 << 1, false));
                } else {
                    self.abbrev(code(fam::TEMPLATE_TEMPLATE, b(name), false));
                }
                self.opt_name(tp.name)?;
                self.strx(s.text)?;
            }
            _ => return Err(bad(node, "templateParams")),
        }
        Ok(())
    }
}
