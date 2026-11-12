//! Global and static-member variables of every shape (`spec/dwarf.md`
//! §6.11, WP-059), as `llc`'s `getOrCreateGlobalVariableDIE` and
//! `addLocationAttribute` write them: a static member's definition with
//! `DW_AT_specification` (made before its class), a declaration, a
//! constant (`DW_AT_const_value` for `DW_OP_constu`/`DW_OP_consts` +
//! `DW_OP_stack_value`), a variable template's parameters, and a
//! thread-local variable, which AArch64's `llc` writes without a
//! location. The plain shape (a definition with an empty expression) stays
//! the core's (§6.5).

use hod_ir::{Arch, DiFlags, DiGlobalVariable, DiId, DiKind, DiNode, DiOp, ExprId};

use crate::Error;
use crate::abbrev::{code, fam};
use crate::die::{Kind, NONE, UnitPlan, Writer, bad, uleb_len, unsupported};
use crate::leb::{write_sleb, write_uleb};
use crate::scope::{
    GLOBAL_EXPRESSION, GLOBAL_SCOPE, GLOBAL_SHAPE, STATIC_MEMBER_DECLARATION, TLS_LOCATION,
    VarShape,
};

const DW_OP_ADDRX: u8 = 0xa1;

impl UnitPlan<'_> {
    /// The constant a global's expression stands for, `(bits, signed)`;
    /// `None` for an empty expression.
    fn global_constant(&self, var: DiId, expr: ExprId) -> Result<Option<(u64, bool)>, Error> {
        if expr == ExprId::EMPTY {
            return Ok(None);
        }
        let mut ops = self.debug.ops(expr);
        let value = match (ops.next(), ops.next(), ops.next()) {
            (Some(DiOp::Constu(v)), Some(DiOp::StackValue), None) => (v, false),
            (Some(DiOp::Consts(v)), Some(DiOp::StackValue), None) => (v, true),
            _ => return Err(unsupported(var, GLOBAL_EXPRESSION)),
        };
        Ok(Some(value))
    }

    /// The hook of `discover_global` (§6.11): a global of a shape the core
    /// does not write gets its DIE here; `None` leaves a plain global (and
    /// a function's `static`) to the core.
    pub(crate) fn discover_scope_global(
        &mut self,
        gve: DiId,
        var: DiId,
        v: &DiGlobalVariable<'_>,
        expr: ExprId,
        chain: &mut Vec<DiId>,
    ) -> Result<Option<u32>, Error> {
        if let Some(s) = v.scope
            && matches!(
                self.debug.find_kind(s),
                Some(DiKind::Subprogram | DiKind::LexicalBlock | DiKind::LexicalBlockFile)
            )
        {
            // A function's thread-local `static` (WP-094, §6.13): under
            // the function's DIE as WP-085 puts it, with the location (or
            // none) of a thread-local global. Any other shape is WP-085's.
            if self.scope_dies.is_tls(gve)
                && expr == ExprId::EMPTY
                && v.is_definition
                && v.declaration.is_none()
                && v.template_params.is_empty()
                && v.ty.is_some()
                && (v.line == 0 || v.file.is_some())
            {
                return self.tls_static(var, v, s).map(Some);
            }
            return Ok(None); // WP-085's
        }
        let tls = self.scope_dies.is_tls(gve);
        if expr == ExprId::EMPTY
            && v.is_definition
            && v.declaration.is_none()
            && v.template_params.is_empty()
            && !tls
        {
            return Ok(None);
        }
        self.global_die(var, v, expr, tls, chain).map(Some)
    }

    /// The DIE of global `var` with expression `expr` (empty for one an
    /// imported entity names), made in `llc`'s order: its context, the
    /// variable, a static member's class and declaration or the type, the
    /// template parameters.
    pub(crate) fn global_die(
        &mut self,
        var: DiId,
        v: &DiGlobalVariable<'_>,
        expr: ExprId,
        tls: bool,
        chain: &mut Vec<DiId>,
    ) -> Result<u32, Error> {
        let constant = self.global_constant(var, expr)?;
        // `llc` writes a constant even for a thread-local variable.
        let tls = tls && constant.is_none();
        if tls && self.scope_dies.arch == Some(Arch::X86_64) {
            return Err(unsupported(var, TLS_LOCATION));
        }
        if v.line != 0 && v.file.is_none() {
            return Err(unsupported(var, GLOBAL_SHAPE));
        }
        let member = v.declaration.is_some();
        if member && !v.is_definition {
            return Err(unsupported(var, GLOBAL_SHAPE));
        }
        if !member && (v.ty.is_none() || v.name.is_empty()) {
            return Err(unsupported(var, GLOBAL_SHAPE));
        }
        let parent = self.scope_parent(var, v.scope, chain).map_err(|e| match e {
            Error::Unsupported { node, .. } => Error::Unsupported { node, what: GLOBAL_SCOPE },
            e => e,
        })?;
        let die = self.push(var, parent, Kind::Variable);
        let mut shape = VarShape { spec: NONE, spec_type: false, constant, tls };
        if let Some(decl) = v.declaration {
            // `getOrCreateStaticMemberDIE`: the class (with its members),
            // then the declaration if the class does not list it.
            let DiNode::DerivedType(d) = self.node_of(var, "declaration", decl)? else {
                return Err(bad(var, "declaration"));
            };
            let class = d.scope.filter(|&c| self.debug.find_kind(c) == Some(DiKind::CompositeType));
            let (Some(class), true) = (class, d.flags.0 & DiFlags::STATIC_MEMBER.0 != 0) else {
                return Err(unsupported(var, STATIC_MEMBER_DECLARATION));
            };
            self.reference_type(var, "declaration", class)?;
            let spec = self.reference_static_member(var, decl)?;
            if self.dies[spec as usize].kind != Kind::StaticMember {
                return Err(unsupported(var, STATIC_MEMBER_DECLARATION));
            }
            shape.spec = spec;
            if let Some(t) = v.ty
                && Some(t) != d.base_type
            {
                shape.spec_type = true;
                self.reference_type(var, "type", t)?;
            }
        } else {
            self.reference_type(var, "type", v.ty.unwrap_or(DiId::NONE))?;
        }
        self.add_template_params(die, v.template_params)?;
        self.scope_dies.vars.push((die, shape));
        Ok(die)
    }

    /// The DIE of a function's thread-local `static` `var`, scoped in
    /// `scope`: `discover_global`'s order (the function's DIE, the
    /// variable, its type), the shape of a thread-local global.
    fn tls_static(
        &mut self,
        var: DiId,
        v: &DiGlobalVariable<'_>,
        scope: DiId,
    ) -> Result<u32, Error> {
        if self.scope_dies.arch == Some(Arch::X86_64) {
            return Err(unsupported(var, TLS_LOCATION));
        }
        let parent = self.static_parent(var, scope)?;
        let die = self.push(var, parent, Kind::Variable);
        let shape = VarShape { spec: NONE, spec_type: false, constant: None, tls: true };
        self.scope_dies.vars.push((die, shape));
        self.reference_type(var, "type", v.ty.unwrap_or(DiId::NONE))?;
        Ok(die)
    }

    /// Whether the global with DIE `die` takes its `.debug_addr` entry: a
    /// constant and an AArch64 thread-local variable do not.
    pub(crate) fn global_takes_address(&self, die: u32) -> bool {
        self.scope_dies.var(die).is_none_or(|s| s.constant.is_none() && !s.tls)
    }
}

impl<'a> Writer<'a> {
    /// The address the code generator gave the global with DIE `die`.
    fn global_addr(&self, die: u32) -> Option<(hod_obj::SymbolId, i64)> {
        let at = *self.addr_of.get(die as usize)?;
        (at != NONE).then(|| self.cur[at as usize])
    }

    /// Writes a global of WP-059's shapes (§6.11).
    pub(crate) fn scope_variable_die(
        &mut self,
        plan: &UnitPlan<'a>,
        die: u32,
        node: DiId,
        v: &DiGlobalVariable<'a>,
        children: bool,
    ) -> Result<(), Error> {
        let b = u32::from;
        let shape = plan.scope_dies.var(die).ok_or(bad(node, "var"))?;
        let addr = self.global_addr(die).filter(|_| shape.constant.is_none() && !shape.tls);
        let (cu, cs) = match shape.constant {
            Some((_, false)) => (true, false),
            Some((_, true)) => (false, true),
            None => (false, false),
        };
        let loc = addr.is_some();
        let link = !v.linkage_name.is_empty();
        let align = v.align != 0;
        if shape.spec == NONE {
            let file = match v.file {
                Some(f) if v.line != 0 => Some(plan.file_index(f).ok_or(bad(node, "file"))?),
                _ => None,
            };
            let ext = !v.is_local;
            let declaration = !v.is_definition;
            if cu && !declaration && !align && !link && !children {
                self.abbrev(code(fam::VARIABLE_CONST, b(ext) | b(file.is_some()) << 1, false));
            } else if !cu && !cs && !declaration && !children {
                let mask =
                    b(ext) | b(file.is_some()) << 1 | b(align) << 2 | b(loc) << 3 | b(link) << 4;
                self.abbrev(code(fam::GLOBAL_VARIABLE, mask, false));
            } else {
                let mask = b(ext)
                    | b(file.is_some()) << 1
                    | b(declaration) << 2
                    | b(align) << 3
                    | b(loc) << 4
                    | b(cu) << 5
                    | b(cs) << 6
                    | b(link) << 7;
                self.abbrev(code(fam::GLOBAL_ANY, mask, children));
            }
            self.strx(v.name)?;
            self.ref_to(plan, node, v.ty.unwrap_or(DiId::NONE))?;
            if let Some(file) = file {
                write_uleb(&mut self.info, u64::from(file));
                write_uleb(&mut self.info, u64::from(v.line));
            }
        } else {
            if !shape.spec_type && !align && !cu && !cs && !children {
                self.abbrev(code(fam::VARIABLE_SPEC, b(loc) | b(link) << 1, false));
            } else {
                let mask = b(shape.spec_type)
                    | b(align) << 1
                    | b(loc) << 2
                    | b(cu) << 3
                    | b(cs) << 4
                    | b(link) << 5;
                self.abbrev(code(fam::VARIABLE_SPEC_ANY, mask, children));
            }
            self.ref4(shape.spec);
            if shape.spec_type {
                self.ref_to(plan, node, v.ty.unwrap_or(DiId::NONE))?;
            }
        }
        if align {
            write_uleb(&mut self.info, u64::from(v.align / 8));
        }
        if let Some((symbol, addend)) = addr {
            let index = self.push_addr(symbol, addend);
            write_uleb(&mut self.info, 1 + uleb_len(u64::from(index)) as u64);
            self.info.push(DW_OP_ADDRX);
            write_uleb(&mut self.info, u64::from(index));
        }
        match shape.constant {
            Some((bits, false)) => write_uleb(&mut self.info, bits),
            Some((bits, true)) => write_sleb(&mut self.info, bits as i64),
            None => {}
        }
        if link {
            self.strx(v.linkage_name)?;
        }
        Ok(())
    }
}
