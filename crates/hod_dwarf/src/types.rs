//! The type graph (`spec/dwarf.md` §6.8): which type DIEs a unit has, in
//! `llc`'s order and under `llc`'s parents, and how each is written.
//!
//! Discovery follows `llc`'s `getOrCreateTypeDIE`: a type's context (its
//! enclosing class, namespace or the unit) is made first, then, if the
//! type still has no DIE, the DIE is appended to the context and its
//! contents are constructed, each reference in turn, depth first. The
//! recursion is an explicit work stack, so a deep or adversarial graph
//! cannot overflow the thread's stack; a type is looked up by its node
//! (`slot`) before anything else, so a cycle through a pointer costs one
//! lookup.

use hod_ir::{Const, DiBound, DiFlags, DiId, DiKind, DiNode, SpFlags};

use crate::Error;
use crate::abbrev::{code, fam, tag};
use crate::die::{Die, Kind, NONE, UnitPlan, Writer, bad, uleb_len, unsupported};
use crate::leb::{write_sleb, write_uleb};

const DW_OP_CONSTU: u8 = 0x10;
const DW_OP_DUP: u8 = 0x12;
const DW_OP_DEREF: u8 = 0x06;
const DW_OP_MINUS: u8 = 0x1c;
const DW_OP_PLUS: u8 = 0x22;
const DW_ATE_UNSIGNED: u8 = 0x07;
const DW_CC_NORMAL: u16 = 0x01;
const DW_CC_PASS_BY_REFERENCE: u8 = 0x04;
const DW_CC_PASS_BY_VALUE: u8 = 0x05;
const DW_VIRTUALITY_VIRTUAL: u8 = 0x01;
const INDEX_TYPE: &[u8] = b"__ARRAY_SIZE_TYPE__";

/// What a node is made into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Want {
    Type,
    StaticMember,
    Method,
}

#[derive(Clone, Copy, Debug)]
enum Task {
    /// `getOrCreate…`: the node's DIE, made with its context if new.
    /// `from` and `field` name the reference, for an error.
    Ensure { from: DiId, field: &'static str, node: DiId, want: Want },
    /// The DIE of a node whose context now exists.
    Create { node: DiId, want: Want },
    /// Step `step` of the construction of DIE `die`.
    Construct { die: u32, step: u32 },
}

/// The planner's scratch while a plan is built; empty afterwards.
#[derive(Clone, Debug, Default)]
pub(crate) struct Build {
    work: Vec<Task>,
    /// Per DIE: its construction is in progress.
    pub(crate) open: Vec<bool>,
    /// `(method DIE, containingType)` of the virtual member functions:
    /// `llc` adds `DW_AT_containing_type` at the end of the unit, only if
    /// that type has a DIE by then.
    virtuals: Vec<(u32, Option<DiId>)>,
}

/// `DW_ACCESS_*` of a node's flags, or 0.
const fn access(flags: DiFlags) -> u8 {
    match flags.0 & 3 {
        1 => 3, // private
        2 => 2, // protected
        3 => 1, // public
        _ => 0,
    }
}

const fn has(flags: DiFlags, f: DiFlags) -> bool {
    flags.0 & f.0 != 0
}

/// `dwarf::isC`: the languages whose subroutine types say
/// `DW_AT_prototyped`.
const fn is_c(language: u16) -> bool {
    matches!(language, 0x01 | 0x02 | 0x0c | 0x10 | 0x1d | 0x2c)
}

/// `llc`'s `getDefaultLowerBound` for DWARF 5: the lower bound a
/// subrange omits, or -1 for a language without one.
const fn default_lower_bound(language: u16) -> i64 {
    match language {
        0x01 | 0x02 | 0x04 | 0x0c | 0x10 | 0x11 | 0x0b | 0x12 | 0x13 | 0x14 | 0x15 | 0x16
        | 0x18 | 0x19 | 0x1a | 0x1b | 0x1c | 0x1d | 0x1e | 0x20 | 0x21 | 0x24 | 0x25 => 0,
        0x03 | 0x05..=0x0a | 0x0d | 0x0e | 0x0f | 0x17 | 0x1f | 0x22 | 0x23 => 1,
        _ => -1,
    }
}

const fn is_type_kind(kind: DiKind) -> bool {
    matches!(
        kind,
        DiKind::BasicType | DiKind::DerivedType | DiKind::CompositeType | DiKind::SubroutineType
    )
}

/// The attributes of a member function declaration, which pick its
/// family.
#[derive(Clone, Copy, Debug, Default)]
struct MethodAttrs {
    link: bool,
    name: bool,
    decl: bool,
    ty: bool,
    virt: bool,
    cont: bool,
    art: bool,
    ext: bool,
    acc: bool,
    expl: bool,
    lref: bool,
    rref: bool,
    nr: bool,
    del: bool,
    /// What no family holds: a calling convention, a virtual function
    /// without a vtable index, `DW_AT_prototyped`, Fortran's flags.
    other: bool,
}

/// The family and mask of a member function declaration, or `None` for
/// a shape no family holds (refused when the plan is built).
fn method_family(a: &MethodAttrs) -> Option<(usize, u32)> {
    let b = u32::from;
    if a.other || a.virt != a.cont {
        return None;
    }
    let qualified = a.lref || a.rref || a.nr;
    if !qualified && !a.expl && !a.del && a.name {
        if !a.virt && !a.art && !a.acc {
            return Some((
                fam::SUBPROGRAM_DECL,
                b(a.link) | b(a.decl) << 1 | b(a.ty) << 2 | b(a.ext) << 3,
            ));
        }
        if !a.virt {
            return Some((
                fam::SPECIAL_DECL,
                b(a.link)
                    | b(a.decl) << 1
                    | b(a.ty) << 2
                    | b(a.art) << 3
                    | b(a.ext) << 4
                    | b(a.acc) << 5,
            ));
        }
        if !a.art && !a.acc {
            return Some((
                fam::VIRTUAL_DECL,
                b(a.link) | b(a.decl) << 1 | b(a.ty) << 2 | b(a.ext) << 3,
            ));
        }
    }
    let common = b(a.link) | b(a.name) << 1 | b(a.decl) << 2 | b(a.ty) << 3 | b(a.virt) << 4;
    if !qualified {
        return Some((
            fam::METHOD,
            common | b(a.art) << 5 | b(a.ext) << 6 | b(a.acc) << 7 | b(a.expl) << 8 | b(a.del) << 9,
        ));
    }
    if !a.expl && !a.art {
        return Some((
            fam::METHOD_QUALIFIED,
            common
                | b(a.ext) << 5
                | b(a.acc) << 6
                | b(a.lref) << 7
                | b(a.rref) << 8
                | b(a.nr) << 9
                | b(a.del) << 10,
        ));
    }
    None
}

impl<'a> UnitPlan<'a> {
    /// The entry point of the type graph (§6.8): type `t`, which `from`'s
    /// field `field` names, has a DIE in this unit, made (with everything
    /// it reaches) if it is new; its index. Later packages call it for
    /// the types their DIEs name (a subprogram's type, a local's), before
    /// the plan is inverted, so that the types land in `llc`'s order.
    ///
    /// # Errors
    /// `BadReference` if `t` is not a type node; `MemberOutsideType`,
    /// `TypeCycle`, `MissingSize`; `Unsupported` (§6.5, §6.8).
    pub(crate) fn reference_type(
        &mut self,
        from: DiId,
        field: &'static str,
        t: DiId,
    ) -> Result<u32, Error> {
        let mut work = std::mem::take(&mut self.build.work);
        work.clear();
        work.push(Task::Ensure { from, field, node: t, want: Want::Type });
        let result = self.drain(&mut work);
        self.build.work = work;
        result?;
        Ok(self.slot[t.index()])
    }

    fn drain(&mut self, work: &mut Vec<Task>) -> Result<(), Error> {
        while let Some(task) = work.pop() {
            match task {
                Task::Ensure { from, field, node, want } => {
                    self.ensure(from, field, node, want, work)?;
                }
                Task::Create { node, want } => self.create(node, want, work)?,
                Task::Construct { die, step } => {
                    if !self.construct(die, step, work)?
                        && let Some(o) = self.build.open.get_mut(die as usize)
                    {
                        *o = false;
                    }
                }
            }
        }
        Ok(())
    }

    /// After discovery: the virtual functions' containing types, the
    /// side tables sorted, and the member function shapes checked.
    pub(crate) fn finish_types(&mut self) -> Result<(), Error> {
        for &(die, ct) in &self.build.virtuals {
            if let Some(t) = ct
                && self.debug.contains(t)
                && self.slot[t.index()] != NONE
            {
                self.aux.push((die, self.slot[t.index()]));
            }
        }
        self.aux.sort_unstable();
        self.values.sort_unstable_by_key(|v| v.0);
        for i in 0..self.dies.len() {
            if self.dies[i].kind == Kind::Method {
                let a = method_attrs(self, i as u32);
                if method_family(&a).is_none() {
                    return Err(unsupported(
                        self.dies[i].node,
                        "a member function declaration with a calling convention, without a vtable index or containing type, or with explicit or artificial and a ref-qualifier or noreturn",
                    ));
                }
            }
        }
        self.build = Build::default();
        Ok(())
    }

    pub(crate) fn aux_of(&self, die: u32) -> u32 {
        match self.aux.binary_search_by_key(&die, |a| a.0) {
            Ok(i) => self.aux[i].1,
            Err(_) => NONE,
        }
    }

    fn value_of(&self, die: u32) -> Option<(u64, bool)> {
        let i = self.values.binary_search_by_key(&die, |v| v.0).ok()?;
        Some((self.values[i].1, self.values[i].2))
    }

    fn scope_field(&self, node: DiId) -> Option<DiId> {
        match self.debug.node(node) {
            DiNode::DerivedType(d) => d.scope,
            DiNode::CompositeType(c) => c.scope,
            DiNode::Subprogram(s) => s.scope,
            _ => None,
        }
    }

    fn ensure(
        &mut self,
        from: DiId,
        field: &'static str,
        t: DiId,
        want: Want,
        work: &mut Vec<Task>,
    ) -> Result<(), Error> {
        let Some(kind) = self.debug.find_kind(t) else { return Err(bad(from, field)) };
        let fits = match want {
            Want::Type => is_type_kind(kind),
            Want::StaticMember => kind == DiKind::DerivedType,
            Want::Method => kind == DiKind::Subprogram,
        };
        if !fits {
            return Err(bad(from, field));
        }
        let d = self.slot[t.index()];
        if d != NONE {
            return self.check_cycle(d, t, work);
        }
        // The context first, from the outermost class without a DIE in
        // (`getOrCreateContextDIE`); a namespace or the unit is found
        // when the DIE is created.
        work.push(Task::Create { node: t, want });
        let mut at = t;
        let mut steps = 0usize;
        while let Some(s) = self.scope_field(at) {
            if self.debug.find_kind(s) != Some(DiKind::CompositeType)
                || self.slot[s.index()] != NONE
            {
                break;
            }
            steps += 1;
            if steps > self.slot.len() {
                return Err(Error::TypeCycle { node: s.raw() });
            }
            work.push(Task::Create { node: s, want: Want::Type });
            at = s;
        }
        Ok(())
    }

    /// A reference to DIE `d`, whose construction may be in progress: a
    /// cycle of typedefs, qualifiers and arrays only is an infinite type.
    fn check_cycle(&self, d: u32, t: DiId, work: &[Task]) -> Result<(), Error> {
        if !self.build.open.get(d as usize).copied().unwrap_or(false) || !self.value_kind(d) {
            return Ok(());
        }
        for task in work.iter().rev() {
            if let Task::Construct { die, .. } = *task {
                if die == d {
                    return Err(Error::TypeCycle { node: t.raw() });
                }
                if !self.value_kind(die) {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// A DIE whose type contains its base by value.
    fn value_kind(&self, die: u32) -> bool {
        let d = self.dies[die as usize];
        match d.kind {
            Kind::Array => true,
            Kind::Derived => match self.debug.node(d.node) {
                DiNode::DerivedType(x) => matches!(
                    x.tag,
                    tag::TYPEDEF
                        | tag::TEMPLATE_ALIAS
                        | tag::CONST_TYPE
                        | tag::VOLATILE_TYPE
                        | tag::RESTRICT_TYPE
                        | tag::ATOMIC_TYPE
                ),
                _ => false,
            },
            _ => false,
        }
    }

    /// The parent DIE of a node whose `scope` is `scope`. A member's
    /// scope must be a class.
    fn context(&mut self, of: DiId, scope: Option<DiId>, member: bool) -> Result<u32, Error> {
        let Some(s) = scope else {
            return if member { Err(Error::MemberOutsideType { node: of.raw() }) } else { Ok(0) };
        };
        let kind = self.debug.find_kind(s).ok_or(bad(of, "scope"))?;
        if member && kind != DiKind::CompositeType {
            return Err(Error::MemberOutsideType { node: of.raw() });
        }
        match kind {
            DiKind::CompileUnit | DiKind::File => Ok(0),
            DiKind::Namespace => {
                let mut chain = Vec::new();
                self.scope_parent(of, scope, &mut chain)
            }
            DiKind::CompositeType => match self.slot[s.index()] {
                NONE => Err(bad(of, "scope")),
                d => Ok(d),
            },
            // A function-local type: WP-057 makes the function's or the
            // block's DIE first, and the type is then its child.
            DiKind::Subprogram | DiKind::LexicalBlock | DiKind::LexicalBlockFile => {
                match self.slot[s.index()] {
                    NONE => Err(unsupported(of, "a function-local type (WP-057)")),
                    d => Ok(d),
                }
            }
            _ => Err(unsupported(of, "a scope that is not a namespace, a class or the unit")),
        }
    }

    fn open(&mut self, die: u32, work: &mut Vec<Task>) {
        let open = &mut self.build.open;
        if open.len() <= die as usize {
            open.resize(die as usize + 1, false);
        }
        open[die as usize] = true;
        work.push(Task::Construct { die, step: 0 });
    }

    fn create(&mut self, t: DiId, want: Want, work: &mut Vec<Task>) -> Result<(), Error> {
        if self.slot[t.index()] != NONE {
            return Ok(()); // made while its context was
        }
        match (want, self.debug.node(t)) {
            (Want::Type, DiNode::BasicType(b)) => {
                if b.tag == tag::UNSPECIFIED_TYPE {
                    self.push(t, 0, Kind::BaseType);
                    return Ok(());
                }
                if b.tag != tag::BASE_TYPE {
                    return Err(unsupported(
                        t,
                        "a basic type that is not a base or unspecified type",
                    ));
                }
                if b.encoding > 0xff {
                    return Err(unsupported(t, "a base type encoding beyond one byte"));
                }
                if b.size == 0 {
                    return Err(Error::MissingSize { node: t.raw() });
                }
                self.push(t, 0, Kind::BaseType);
            }
            (Want::Type, DiNode::DerivedType(d)) => {
                self.check_derived(t, &d)?;
                let parent = self.context(t, d.scope, false)?;
                let die = self.push(t, parent, Kind::Derived);
                self.open(die, work);
            }
            (Want::Type, DiNode::CompositeType(c)) => {
                let kind = match c.tag {
                    tag::STRUCTURE_TYPE | tag::CLASS_TYPE | tag::UNION_TYPE => Kind::Composite,
                    tag::ENUMERATION_TYPE => Kind::Enumeration,
                    tag::ARRAY_TYPE => Kind::Array,
                    _ => return Err(unsupported(t, "a composite type of this tag")),
                };
                self.check_composite(t, kind, &c)?;
                let parent = self.context(t, c.scope, false)?;
                let die = self.push(t, parent, kind);
                self.open(die, work);
            }
            (Want::Type, DiNode::SubroutineType(_)) => {
                let die = self.push(t, 0, Kind::Subroutine);
                self.open(die, work);
            }
            (Want::Type, _) => return Err(unsupported(t, "a type of this kind")),
            (Want::StaticMember, DiNode::DerivedType(d)) => {
                if !matches!(d.tag, tag::MEMBER | tag::VARIABLE) {
                    return Err(unsupported(t, "a static member that is not a member or variable"));
                }
                if d.name.is_empty() || d.base_type.is_none() {
                    return Err(unsupported(t, "a static member without a name or a type"));
                }
                let parent = self.context(t, d.scope, true)?;
                let die = self.push(t, parent, Kind::StaticMember);
                if let Some(c) = d.extra_data
                    && let Some(v) = self.constant(t, c, d.base_type.unwrap_or(DiId::NONE))?
                {
                    self.values.push((die, v.0, v.1));
                }
                self.open(die, work);
            }
            (Want::Method, DiNode::Subprogram(sp)) => {
                if has_sp(sp.sp_flags, SpFlags::DEFINITION) {
                    return Err(unsupported(t, "a function definition among a class's elements"));
                }
                if sp.declaration.is_some() || !sp.thrown_types.is_empty() {
                    return Err(unsupported(
                        t,
                        "a member function declaration with a declaration or thrown types",
                    ));
                }
                if let Some(ty) = sp.ty
                    && self.debug.find_kind(ty) != Some(DiKind::SubroutineType)
                {
                    return Err(bad(t, "type"));
                }
                let parent = self.context(t, sp.scope, true)?;
                let die = self.push(t, parent, Kind::Method);
                if sp.sp_flags.0 & 3 != 0 {
                    self.build.virtuals.push((die, sp.containing_type));
                }
                self.open(die, work);
            }
            _ => unreachable!("`ensure` checked the node's kind"),
        }
        Ok(())
    }

    fn check_derived(&self, t: DiId, d: &hod_ir::DiDerivedType<'_>) -> Result<(), Error> {
        if d.dwarf_address_space.is_some() {
            return Err(unsupported(t, "a pointer with an address space"));
        }
        let line = d.line != 0 && !has(d.flags, DiFlags::FWD_DECL);
        let acc = access(d.flags) != 0;
        match d.tag {
            tag::POINTER_TYPE => {
                if acc || line {
                    return Err(unsupported(t, "a pointer with accessibility or a line"));
                }
            }
            tag::REFERENCE_TYPE
            | tag::RVALUE_REFERENCE_TYPE
            | tag::CONST_TYPE
            | tag::VOLATILE_TYPE
            | tag::RESTRICT_TYPE
            | tag::ATOMIC_TYPE => {
                let sized = d.size != 0
                    && !matches!(d.tag, tag::REFERENCE_TYPE | tag::RVALUE_REFERENCE_TYPE);
                if !d.name.is_empty() || sized || acc || line {
                    return Err(unsupported(
                        t,
                        "a qualified or reference type with a name, size, accessibility or line",
                    ));
                }
            }
            tag::TYPEDEF | tag::TEMPLATE_ALIAS => {
                if d.name.is_empty() || d.size != 0 {
                    return Err(unsupported(t, "a typedef without a name or with a size"));
                }
            }
            tag::PTR_TO_MEMBER_TYPE => {
                if d.base_type.is_none() {
                    return Err(bad(t, "baseType"));
                }
                if !d.extra_data.is_some_and(|c| self.debug.find_kind(c).is_some_and(is_type_kind))
                {
                    return Err(bad(t, "extraData"));
                }
                if !d.name.is_empty() || acc || line {
                    return Err(unsupported(
                        t,
                        "a pointer to member with a name, accessibility or line",
                    ));
                }
            }
            tag::MEMBER | tag::INHERITANCE | tag::FRIEND | tag::VARIABLE => {
                return Err(unsupported(t, "a member used as a type"));
            }
            _ => return Err(unsupported(t, "a derived type of this tag")),
        }
        Ok(())
    }

    fn check_composite(
        &self,
        t: DiId,
        kind: Kind,
        c: &hod_ir::DiCompositeType<'_>,
    ) -> Result<(), Error> {
        if c.runtime_lang != 0 {
            return Err(unsupported(t, "a composite type with a runtime language"));
        }
        let fwd = has(c.flags, DiFlags::FWD_DECL);
        match kind {
            Kind::Composite if fwd => {
                let cc = has(c.flags, DiFlags::TYPE_PASS_BY_VALUE)
                    || has(c.flags, DiFlags::TYPE_PASS_BY_REFERENCE);
                if has(c.flags, DiFlags::EXPORT_SYMBOLS)
                    || c.vtable_holder.is_some()
                    || cc
                    || c.align != 0
                {
                    return Err(unsupported(
                        t,
                        "a declared class with exported symbols, a vtable holder, a calling convention or an alignment",
                    ));
                }
            }
            Kind::Enumeration if !fwd && c.size == 0 => {
                return Err(Error::MissingSize { node: t.raw() });
            }
            Kind::Enumeration => {
                if let Some(b) = c.base_type {
                    if !self.debug.find_kind(b).is_some_and(is_type_kind) {
                        return Err(bad(t, "baseType"));
                    }
                    self.is_unsigned(b)?;
                }
            }
            Kind::Array => {
                if c.base_type.is_none() {
                    return Err(bad(t, "baseType"));
                }
                if has(c.flags, DiFlags::VECTOR) {
                    if c.size == 0 {
                        return Err(Error::MissingSize { node: t.raw() });
                    }
                    let one = c.elements.len() == 1
                        && self.debug.find_kind(c.elements[0]) == Some(DiKind::Subrange);
                    if !one {
                        return Err(unsupported(t, "a vector without exactly one subrange"));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// `isUnsignedDIType`: whether a constant of type `t` is written
    /// unsigned.
    fn is_unsigned(&self, mut t: DiId) -> Result<bool, Error> {
        for _ in 0..=self.slot.len() {
            if !self.debug.contains(t) {
                return Err(bad(t, "baseType"));
            }
            match self.debug.node(t) {
                DiNode::CompositeType(c) => return Ok(c.tag != tag::ENUMERATION_TYPE),
                DiNode::DerivedType(d) => match d.tag {
                    tag::POINTER_TYPE
                    | tag::PTR_TO_MEMBER_TYPE
                    | tag::REFERENCE_TYPE
                    | tag::RVALUE_REFERENCE_TYPE => return Ok(true),
                    _ => t = d.base_type.ok_or(bad(t, "baseType"))?,
                },
                DiNode::BasicType(b) => {
                    return Ok(b.tag == tag::UNSPECIFIED_TYPE
                        || matches!(b.encoding, 0x02 | 0x07 | 0x08 | 0x0e | 0x10));
                }
                _ => return Err(bad(t, "baseType")),
            }
        }
        Err(Error::TypeCycle { node: t.raw() })
    }

    /// A static member's value: `(bits, signed)`, or `None` for a
    /// constant `llc` does not write (not an integer or a float).
    fn constant(&self, member: DiId, c: DiId, ty: DiId) -> Result<Option<(u64, bool)>, Error> {
        let DiNode::Constant(k) = self.node_of(member, "extraData", c)? else {
            return Err(bad(member, "extraData"));
        };
        let Some((consts, types)) = self.consts else {
            return Err(unsupported(
                member,
                "a static member's value without the module's constants (UnitPlan::of_module)",
            ));
        };
        let unsigned = self.is_unsigned(ty)?;
        Ok(match consts.get(k.value) {
            Const::Int { ty: it, value } => {
                if unsigned {
                    Some((value, false))
                } else {
                    let bits = types.int_bits(it).unwrap_or(64).clamp(1, 64);
                    let shift = 64 - bits;
                    Some(((((value << shift) as i64) >> shift) as u64, true))
                }
            }
            Const::Float { bits, .. } => Some((bits, false)),
            Const::WideInt { .. } | Const::WideFloat { .. } => {
                return Err(unsupported(member, "a static member's value wider than 64 bits"));
            }
            _ => None,
        })
    }

    /// Pushes the next step of DIE `die`, then `task`.
    fn then(work: &mut Vec<Task>, die: u32, step: u32, task: Option<Task>) {
        work.push(Task::Construct { die, step });
        if let Some(t) = task {
            work.push(t);
        }
    }

    /// Step `step` of DIE `die`'s construction; false when it is done.
    fn construct(&mut self, die: u32, step: u32, work: &mut Vec<Task>) -> Result<bool, Error> {
        let Die { node, kind, .. } = self.dies[die as usize];
        let ensure = |field, t: DiId| Task::Ensure { from: node, field, node: t, want: Want::Type };
        match (kind, self.debug.node(node)) {
            (Kind::Derived, DiNode::DerivedType(d)) => match step {
                0 => Self::then(work, die, 1, d.base_type.map(|b| ensure("baseType", b))),
                1 if d.tag == tag::PTR_TO_MEMBER_TYPE => {
                    Self::then(work, die, 2, d.extra_data.map(|c| ensure("extraData", c)));
                }
                _ => return Ok(false),
            },
            (Kind::StaticMember, DiNode::DerivedType(d)) => match step {
                0 => Self::then(work, die, 1, d.base_type.map(|b| ensure("baseType", b))),
                _ => return Ok(false),
            },
            (Kind::Composite, DiNode::CompositeType(c)) => {
                let n = c.elements.len() as u32;
                if step < n {
                    let e = c.elements[step as usize];
                    let task = self.element(node, die, e)?;
                    Self::then(work, die, step + 1, task);
                } else if step == n {
                    Self::then(
                        work,
                        die,
                        n + 1,
                        c.vtable_holder.map(|v| ensure("vtableHolder", v)),
                    );
                } else {
                    return Ok(false);
                }
            }
            (Kind::Enumeration, DiNode::CompositeType(c)) => match step {
                0 => Self::then(work, die, 1, c.base_type.map(|b| ensure("baseType", b))),
                1 => {
                    for &e in c.elements {
                        if e == DiId::NONE {
                            continue;
                        }
                        if self.debug.find_kind(e) != Some(DiKind::Enumerator) {
                            return Err(bad(node, "elements"));
                        }
                        self.push_anon(e, die, Kind::Enumerator);
                    }
                    return Ok(false);
                }
                _ => return Ok(false),
            },
            (Kind::Array, DiNode::CompositeType(c)) => match step {
                0 => Self::then(work, die, 1, c.base_type.map(|b| ensure("baseType", b))),
                1 => {
                    if self.index_die == NONE {
                        self.index_die = self.push_anon(self.unit, 0, Kind::IndexType);
                    }
                    for &e in c.elements {
                        if e == DiId::NONE {
                            continue;
                        }
                        let DiNode::Subrange(r) = self.node_of(node, "elements", e)? else {
                            return Err(unsupported(e, "an array element that is not a subrange"));
                        };
                        let target = self.check_subrange(e, &r)?;
                        let sub = self.push_anon(e, die, Kind::Subrange);
                        if target != NONE {
                            self.aux.push((sub, target));
                        }
                    }
                    return Ok(false);
                }
                _ => return Ok(false),
            },
            (Kind::Subroutine, DiNode::SubroutineType(s)) => {
                return self.signature(node, die, s.types, step, work);
            }
            (Kind::Method, DiNode::Subprogram(sp)) => {
                let types = match sp.ty.map(|t| self.debug.node(t)) {
                    Some(DiNode::SubroutineType(s)) => s.types,
                    _ => &[],
                };
                return self.signature(node, die, types, step, work);
            }
            (Kind::Member | Kind::Friend, DiNode::DerivedType(d)) => match step {
                0 => Self::then(work, die, 1, d.base_type.map(|b| ensure("baseType", b))),
                _ => return Ok(false),
            },
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// A subroutine type's or a member function's return type, then its
    /// parameters (`constructSubprogramArguments`).
    fn signature(
        &mut self,
        node: DiId,
        die: u32,
        types: &[DiId],
        step: u32,
        work: &mut Vec<Task>,
    ) -> Result<bool, Error> {
        let n = types.len() as u32;
        if step >= n.max(1) {
            return Ok(false);
        }
        let t = types.get(step as usize).copied().unwrap_or(DiId::NONE);
        if step == 0 {
            let task = (t != DiId::NONE).then_some(Task::Ensure {
                from: node,
                field: "types",
                node: t,
                want: Want::Type,
            });
            Self::then(work, die, 1, task);
        } else if t == DiId::NONE {
            if step + 1 != n {
                return Err(unsupported(node, "a null parameter type before the last"));
            }
            self.push_anon(node, die, Kind::Unspecified);
            return Ok(false);
        } else {
            self.push_anon(t, die, Kind::Param);
            let task = Task::Ensure { from: node, field: "types", node: t, want: Want::Type };
            Self::then(work, die, step + 1, Some(task));
        }
        Ok(true)
    }

    /// Element `e` of class DIE `die` (node `class`): a member function,
    /// friend, static member, member or base; the task it needs.
    fn element(&mut self, class: DiId, die: u32, e: DiId) -> Result<Option<Task>, Error> {
        if e == DiId::NONE {
            return Ok(None);
        }
        let node = self.node_of(class, "elements", e)?;
        let ensure =
            |t: DiId| Task::Ensure { from: e, field: "baseType", node: t, want: Want::Type };
        match node {
            DiNode::Subprogram(_) => Ok(Some(Task::Ensure {
                from: class,
                field: "elements",
                node: e,
                want: Want::Method,
            })),
            DiNode::DerivedType(d) if d.tag == tag::FRIEND => {
                let base = d.base_type.ok_or(bad(e, "baseType"))?;
                self.push_anon(e, die, Kind::Friend);
                Ok(Some(ensure(base)))
            }
            DiNode::DerivedType(d) if has(d.flags, DiFlags::STATIC_MEMBER) => {
                Ok(Some(Task::Ensure {
                    from: class,
                    field: "elements",
                    node: e,
                    want: Want::StaticMember,
                }))
            }
            DiNode::DerivedType(d) if matches!(d.tag, tag::MEMBER | tag::INHERITANCE) => {
                // `llc` ignores a member's scope; clang gives a vtable
                // pointer the file.
                if let Some(s) = d.scope
                    && !self.debug.find_kind(s).is_some_and(|k| {
                        is_type_kind(k) || matches!(k, DiKind::File | DiKind::CompileUnit)
                    })
                {
                    return Err(Error::MemberOutsideType { node: e.raw() });
                }
                let base = d.base_type.ok_or(bad(e, "baseType"))?;
                let bitfield = has(d.flags, DiFlags::BIT_FIELD);
                if bitfield && d.size == 0 {
                    return Err(Error::MissingSize { node: e.raw() });
                }
                if d.tag == tag::INHERITANCE
                    && (!d.name.is_empty()
                        || d.line != 0
                        || bitfield
                        || d.align != 0
                        || has(d.flags, DiFlags::ARTIFICIAL))
                {
                    return Err(unsupported(
                        e,
                        "an inheritance with a name, line, bit field, alignment or artificial flag",
                    ));
                }
                self.push_anon(e, die, Kind::Member);
                Ok(Some(ensure(base)))
            }
            DiNode::DerivedType(_) => Err(unsupported(e, "a class element of this tag")),
            _ => Err(unsupported(e, "a class element of this kind")),
        }
    }

    /// Checks a subrange; the DIE of the variable its count names, or
    /// `NONE`.
    fn check_subrange(&self, e: DiId, r: &hod_ir::DiSubrange) -> Result<u32, Error> {
        if r.stride != DiBound::None {
            return Err(unsupported(e, "a subrange with a stride"));
        }
        for b in [r.lower_bound, r.upper_bound] {
            if !matches!(b, DiBound::None | DiBound::Const(_)) {
                return Err(unsupported(e, "a subrange bound that is a variable or an expression"));
            }
        }
        match r.count {
            DiBound::Const(c) if c < -1 => Err(unsupported(e, "a negative subrange count")),
            DiBound::Node(v) => {
                if !matches!(
                    self.debug.find_kind(v),
                    Some(DiKind::LocalVariable | DiKind::GlobalVariable)
                ) {
                    return Err(bad(e, "count"));
                }
                let target = self.slot[v.index()];
                if target != NONE
                    && (r.lower_bound != DiBound::None || r.upper_bound != DiBound::None)
                {
                    return Err(unsupported(e, "a variable count with a bound"));
                }
                Ok(target)
            }
            DiBound::Expr(_) => Err(unsupported(e, "a subrange count that is an expression")),
            _ => Ok(NONE),
        }
    }
}

const fn has_sp(flags: SpFlags, f: SpFlags) -> bool {
    flags.0 & f.0 != 0
}

/// The attributes of member function DIE `die`.
fn method_attrs(plan: &UnitPlan<'_>, die: u32) -> MethodAttrs {
    let d = plan.dies[die as usize];
    let DiNode::Subprogram(sp) = plan.debug.node(d.node) else { return MethodAttrs::default() };
    let (ret, cc) = match sp.ty.map(|t| plan.debug.node(t)) {
        Some(DiNode::SubroutineType(s)) => {
            (s.types.first().is_some_and(|&t| t != DiId::NONE), s.cc != 0 && s.cc != DW_CC_NORMAL)
        }
        _ => (false, false),
    };
    let virtuality = sp.sp_flags.0 & 3 != 0;
    let fortran =
        SpFlags::PURE.0 | SpFlags::ELEMENTAL.0 | SpFlags::RECURSIVE.0 | SpFlags::MAIN_SUBPROGRAM.0;
    MethodAttrs {
        link: !sp.linkage_name.is_empty(),
        name: !sp.name.is_empty(),
        decl: sp.line != 0,
        ty: ret,
        virt: virtuality,
        cont: virtuality && plan.aux_of(die) != NONE,
        art: has(sp.flags, DiFlags::ARTIFICIAL),
        ext: !has_sp(sp.sp_flags, SpFlags::LOCAL_TO_UNIT),
        acc: access(sp.flags) != 0,
        expl: has(sp.flags, DiFlags::EXPLICIT),
        lref: has(sp.flags, DiFlags::LVALUE_REFERENCE),
        rref: has(sp.flags, DiFlags::RVALUE_REFERENCE),
        nr: has(sp.flags, DiFlags::NO_RETURN),
        del: has_sp(sp.sp_flags, SpFlags::DELETED),
        other: cc
            || (virtuality && sp.virtual_index == u32::MAX)
            || sp.sp_flags.0 & fortran != 0
            || (has(sp.flags, DiFlags::PROTOTYPED) && is_c(plan.cu.language)),
    }
}

/// The `flags` of a type node.
fn type_flags(plan: &UnitPlan<'_>, t: DiId) -> DiFlags {
    match plan.debug.node(t) {
        DiNode::BasicType(b) => b.flags,
        DiNode::DerivedType(d) => d.flags,
        DiNode::CompositeType(c) => c.flags,
        DiNode::SubroutineType(s) => s.flags,
        _ => DiFlags(0),
    }
}

/// The `size` field of a type node.
fn type_size(plan: &UnitPlan<'_>, t: DiId) -> u64 {
    match plan.debug.node(t) {
        DiNode::BasicType(b) => b.size,
        DiNode::DerivedType(d) => d.size,
        DiNode::CompositeType(c) => c.size,
        _ => 0,
    }
}

impl<'a> Writer<'a> {
    /// `DW_AT_decl_file` and `DW_AT_decl_line`, when `line` is set.
    fn decl_of(
        plan: &UnitPlan<'_>,
        node: DiId,
        file: Option<DiId>,
        line: u32,
    ) -> Result<Option<(u32, u32)>, Error> {
        if line == 0 {
            return Ok(None);
        }
        let f = file.and_then(|f| plan.file_index(f)).ok_or(bad(node, "file"))?;
        Ok(Some((f, line)))
    }

    fn decl(&mut self, decl: Option<(u32, u32)>) {
        if let Some((f, l)) = decl {
            write_uleb(&mut self.info, u64::from(f));
            write_uleb(&mut self.info, u64::from(l));
        }
    }

    fn opt_byte(&mut self, b: u8) {
        if b != 0 {
            self.info.push(b);
        }
    }

    fn opt_uleb(&mut self, v: u64) {
        if v != 0 {
            write_uleb(&mut self.info, v);
        }
    }

    fn opt_name(&mut self, name: &'a [u8]) -> Result<(), Error> {
        if !name.is_empty() {
            self.strx(name)?;
        }
        Ok(())
    }

    fn opt_ref(&mut self, plan: &UnitPlan<'_>, from: DiId, t: Option<DiId>) -> Result<(), Error> {
        if let Some(t) = t {
            self.ref_to(plan, from, t)?;
        }
        Ok(())
    }

    /// Writes a DIE of the type graph (§6.8).
    #[allow(clippy::too_many_lines)]
    pub(crate) fn type_die(
        &mut self,
        plan: &UnitPlan<'a>,
        die: u32,
        d: Die,
        node: DiNode<'a>,
        children: bool,
    ) -> Result<(), Error> {
        let b = u32::from;
        let n = d.node;
        match (d.kind, node) {
            (Kind::BaseType, DiNode::BasicType(t)) if t.tag == tag::UNSPECIFIED_TYPE => {
                self.abbrev(code(fam::UNSPECIFIED_TYPE, b(!t.name.is_empty()), false));
                self.opt_name(t.name)?;
            }
            (Kind::BaseType, DiNode::BasicType(t)) => {
                let end = if has(t.flags, DiFlags::BIG_ENDIAN) {
                    1
                } else if has(t.flags, DiFlags::LITTLE_ENDIAN) {
                    2
                } else {
                    0
                };
                let bits = t.data_size != 0 && u64::from(t.data_size) != t.size;
                if !t.name.is_empty() && end == 0 && !bits {
                    self.abbrev(code(fam::BASE_TYPE, 0, false));
                } else {
                    let mask = b(!t.name.is_empty()) | b(end != 0) << 1 | b(bits) << 2;
                    self.abbrev(code(fam::BASE_TYPE_ANY, mask, false));
                }
                self.opt_name(t.name)?;
                self.info.push(t.encoding as u8);
                write_uleb(&mut self.info, t.size / 8);
                self.opt_byte(end);
                if bits {
                    write_uleb(&mut self.info, u64::from(t.data_size));
                }
            }
            (Kind::IndexType, _) => {
                self.abbrev(code(fam::INDEX_TYPE, 0, false));
                self.strx(INDEX_TYPE)?;
                write_uleb(&mut self.info, 8);
                self.info.push(DW_ATE_UNSIGNED);
            }
            (Kind::Derived, DiNode::DerivedType(t)) => {
                let acc = access(t.flags);
                let line = if has(t.flags, DiFlags::FWD_DECL) { 0 } else { t.line };
                let decl = Self::decl_of(plan, n, t.file, line)?;
                let ty = t.base_type;
                match t.tag {
                    tag::POINTER_TYPE => {
                        let mask = b(ty.is_some()) | b(!t.name.is_empty()) << 1;
                        self.abbrev(code(fam::POINTER_TYPE, mask, false));
                        self.opt_ref(plan, n, ty)?;
                        self.opt_name(t.name)?;
                    }
                    tag::TYPEDEF if acc == 0 => {
                        let mask = b(ty.is_some()) | b(decl.is_some()) << 1 | b(t.align != 0) << 2;
                        self.abbrev(code(fam::TYPEDEF, mask, false));
                        self.opt_ref(plan, n, ty)?;
                        self.strx(t.name)?;
                        self.decl(decl);
                        self.opt_uleb(u64::from(t.align / 8));
                    }
                    tag::TYPEDEF => {
                        let mask = b(ty.is_some())
                            | b(decl.is_some()) << 1
                            | b(t.align != 0) << 2
                            | 1 << 3;
                        self.abbrev(code(fam::TYPEDEF_ANY, mask, false));
                        self.opt_ref(plan, n, ty)?;
                        self.strx(t.name)?;
                        self.opt_uleb(u64::from(t.align / 8));
                        self.info.push(acc);
                        self.decl(decl);
                    }
                    tag::TEMPLATE_ALIAS => {
                        let mask = b(ty.is_some()) | b(decl.is_some()) << 1 | b(acc != 0) << 2;
                        self.abbrev(code(fam::TEMPLATE_ALIAS, mask, false));
                        self.opt_ref(plan, n, ty)?;
                        self.strx(t.name)?;
                        self.opt_byte(acc);
                        self.decl(decl);
                    }
                    tag::PTR_TO_MEMBER_TYPE => {
                        self.abbrev(code(fam::PTR_TO_MEMBER_TYPE, 0, false));
                        self.opt_ref(plan, n, ty)?;
                        self.opt_ref(plan, n, t.extra_data)?;
                    }
                    other => {
                        let family = match other {
                            tag::CONST_TYPE => fam::CONST_TYPE,
                            tag::REFERENCE_TYPE => fam::REFERENCE_TYPE,
                            tag::RVALUE_REFERENCE_TYPE => fam::RVALUE_REFERENCE_TYPE,
                            tag::VOLATILE_TYPE => fam::VOLATILE_TYPE,
                            tag::RESTRICT_TYPE => fam::RESTRICT_TYPE,
                            _ => fam::ATOMIC_TYPE,
                        };
                        self.abbrev(code(family, b(ty.is_some()), false));
                        self.opt_ref(plan, n, ty)?;
                    }
                }
            }
            (Kind::Member, DiNode::DerivedType(m)) => self.member(plan, n, &m)?,
            (Kind::StaticMember, DiNode::DerivedType(m)) => {
                let decl = Self::decl_of(plan, n, m.file, m.line)?;
                let acc = access(m.flags);
                let art = has(m.flags, DiFlags::ARTIFICIAL);
                let value = plan.value_of(die);
                let reserved =
                    m.tag == tag::VARIABLE && !art && m.align == 0 && value.is_none_or(|v| v.1);
                if reserved {
                    let mask =
                        b(decl.is_some()) | 1 << 1 | b(acc != 0) << 2 | b(value.is_some()) << 3;
                    self.abbrev(code(fam::STATIC_MEMBER, mask, false));
                } else {
                    let family = if m.tag == tag::VARIABLE {
                        fam::STATIC_VARIABLE_ANY
                    } else {
                        fam::STATIC_MEMBER_ANY
                    };
                    let mask = b(decl.is_some())
                        | b(art) << 1
                        | b(acc != 0) << 2
                        | b(value.is_some_and(|v| !v.1)) << 3
                        | b(value.is_some_and(|v| v.1)) << 4
                        | b(m.align != 0) << 5;
                    self.abbrev(code(family, mask, false));
                }
                self.strx(m.name)?;
                self.opt_ref(plan, n, m.base_type)?;
                self.decl(decl);
                self.opt_byte(acc);
                match value {
                    Some((v, true)) => write_sleb(&mut self.info, v as i64),
                    Some((v, false)) => write_uleb(&mut self.info, v),
                    None => {}
                }
                if !reserved {
                    self.opt_uleb(u64::from(m.align / 8));
                }
            }
            (Kind::Friend, DiNode::DerivedType(f)) => {
                self.abbrev(code(fam::FRIEND, 0, false));
                self.opt_ref(plan, n, f.base_type)?;
            }
            (Kind::Composite, DiNode::CompositeType(c)) => {
                let t = match c.tag {
                    tag::STRUCTURE_TYPE => 0,
                    tag::CLASS_TYPE => 1,
                    _ => 2,
                };
                let acc = access(c.flags);
                if has(c.flags, DiFlags::FWD_DECL) {
                    if acc == 0 {
                        self.abbrev(code(fam::STRUCTURE_DECL + t, b(!c.name.is_empty()), children));
                        self.opt_name(c.name)?;
                    } else {
                        let mask = b(!c.name.is_empty()) | 1 << 1;
                        self.abbrev(code(fam::STRUCTURE_DECL_ANY + t, mask, children));
                        self.opt_name(c.name)?;
                        self.info.push(acc);
                    }
                    return Ok(());
                }
                let export = has(c.flags, DiFlags::EXPORT_SYMBOLS);
                let holder = c.vtable_holder.and_then(|v| plan.die_of(v));
                let cc = if has(c.flags, DiFlags::TYPE_PASS_BY_VALUE) {
                    DW_CC_PASS_BY_VALUE
                } else if has(c.flags, DiFlags::TYPE_PASS_BY_REFERENCE) {
                    DW_CC_PASS_BY_REFERENCE
                } else {
                    0
                };
                let decl = Self::decl_of(plan, n, c.file, c.line)?;
                let name = !c.name.is_empty();
                if !export && acc == 0 && c.align == 0 {
                    let mask = b(holder.is_some())
                        | b(cc != 0) << 1
                        | b(name) << 2
                        | b(decl.is_some()) << 3;
                    self.abbrev(code(fam::STRUCTURE_TYPE + t, mask, children));
                } else {
                    let mask = b(export)
                        | b(holder.is_some()) << 1
                        | b(cc != 0) << 2
                        | b(name) << 3
                        | b(acc != 0) << 4
                        | b(decl.is_some()) << 5
                        | b(c.align != 0) << 6;
                    self.abbrev(code(fam::STRUCTURE_ANY + t, mask, children));
                }
                if let Some(h) = holder {
                    self.ref4(h);
                }
                self.opt_byte(cc);
                self.opt_name(c.name)?;
                write_uleb(&mut self.info, c.size / 8);
                self.opt_byte(acc);
                self.decl(decl);
                self.opt_uleb(u64::from(c.align / 8));
            }
            (Kind::Enumeration, DiNode::CompositeType(c)) => {
                let fwd = has(c.flags, DiFlags::FWD_DECL);
                let ty = c.base_type;
                let class = ty.is_some() && has(c.flags, DiFlags::ENUM_CLASS);
                let size = if c.size != 0 {
                    Some(c.size / 8)
                } else if !fwd {
                    Some(0)
                } else {
                    None
                };
                let acc = access(c.flags);
                let decl = if fwd { None } else { Self::decl_of(plan, n, c.file, c.line)? };
                let name = !c.name.is_empty();
                if size.is_some() && !fwd && acc == 0 && c.align == 0 {
                    let mask =
                        b(ty.is_some()) | b(class) << 1 | b(name) << 2 | b(decl.is_some()) << 3;
                    self.abbrev(code(fam::ENUMERATION_TYPE, mask, children));
                } else {
                    let mask = b(ty.is_some())
                        | b(class) << 1
                        | b(name) << 2
                        | b(size.is_some()) << 3
                        | b(fwd) << 4
                        | b(acc != 0) << 5
                        | b(decl.is_some()) << 6
                        | b(c.align != 0) << 7;
                    self.abbrev(code(fam::ENUMERATION_ANY, mask, children));
                }
                self.opt_ref(plan, n, ty)?;
                self.opt_name(c.name)?;
                if let Some(s) = size {
                    write_uleb(&mut self.info, s);
                }
                self.opt_byte(acc);
                self.decl(decl);
                self.opt_uleb(u64::from(c.align / 8));
            }
            (Kind::Enumerator, DiNode::Enumerator(e)) => {
                let parent = plan.dies[d.parent as usize].node;
                let unsigned = match plan.debug.node(parent) {
                    DiNode::CompositeType(c) => match c.base_type {
                        Some(t) => plan.is_unsigned(t)?,
                        None => false,
                    },
                    _ => false,
                };
                self.abbrev(code(fam::ENUMERATOR, if unsigned { 1 } else { 2 }, false));
                self.strx(e.name)?;
                if unsigned {
                    write_uleb(&mut self.info, e.value as u64);
                } else {
                    write_sleb(&mut self.info, e.value);
                }
            }
            (Kind::Array, DiNode::CompositeType(c)) => {
                let vector = has(c.flags, DiFlags::VECTOR);
                let padded = vector && {
                    let count = match c.elements.first().map(|&e| plan.debug.node(e)) {
                        Some(DiNode::Subrange(r)) => match r.count {
                            DiBound::Const(k) => k.max(0) as u64,
                            _ => 0,
                        },
                        _ => 0,
                    };
                    let each = c.base_type.map_or(0, |t| type_size(plan, t));
                    c.size != count.wrapping_mul(each)
                };
                let name = !c.name.is_empty();
                if !vector && !name && children {
                    self.abbrev(code(fam::ARRAY_TYPE, 0, true));
                } else {
                    let mask = b(vector) | b(padded) << 1 | b(name) << 2;
                    self.abbrev(code(fam::ARRAY_ANY, mask, children));
                }
                if padded {
                    write_uleb(&mut self.info, c.size / 8);
                }
                self.opt_ref(plan, n, c.base_type)?;
                self.opt_name(c.name)?;
            }
            (Kind::Subrange, DiNode::Subrange(r)) => {
                let var = plan.aux_of(die);
                if var != NONE {
                    self.abbrev(code(fam::SUBRANGE_VAR_COUNT, 0, false));
                    self.ref4(plan.index_die);
                    self.ref4(var);
                    return Ok(());
                }
                let default = default_lower_bound(plan.cu.language);
                let lower = match r.lower_bound {
                    DiBound::Const(v) if default == -1 || v != default => Some(v),
                    _ => None,
                };
                let count = match r.count {
                    DiBound::Const(v) if v != -1 => Some(v as u64),
                    _ => None,
                };
                let upper = match r.upper_bound {
                    DiBound::Const(v) => Some(v),
                    _ => None,
                };
                let mask = b(count.is_some()) | b(lower.is_some()) << 1 | b(upper.is_some()) << 2;
                self.abbrev(code(fam::SUBRANGE, mask, false));
                self.ref4(plan.index_die);
                if let Some(c) = count {
                    write_uleb(&mut self.info, c);
                }
                if let Some(l) = lower {
                    write_sleb(&mut self.info, l);
                }
                if let Some(u) = upper {
                    write_sleb(&mut self.info, u);
                }
            }
            (Kind::Subroutine, DiNode::SubroutineType(s)) => {
                let ret = s.types.first().copied().filter(|&t| t != DiId::NONE);
                let unprototyped = s.types.len() == 2 && s.types[1] == DiId::NONE;
                let proto = !unprototyped && is_c(plan.cu.language);
                let cc = if s.cc != 0 && s.cc != DW_CC_NORMAL { s.cc as u8 } else { 0 };
                let lref = has(s.flags, DiFlags::LVALUE_REFERENCE);
                let rref = has(s.flags, DiFlags::RVALUE_REFERENCE);
                if !proto && cc == 0 && !lref && !rref {
                    self.abbrev(code(fam::SUBROUTINE_TYPE, b(ret.is_some()), children));
                } else {
                    let mask = b(ret.is_some())
                        | b(proto) << 1
                        | b(cc != 0) << 2
                        | b(lref) << 3
                        | b(rref) << 4;
                    self.abbrev(code(fam::SUBROUTINE_ANY, mask, children));
                }
                self.opt_ref(plan, n, ret)?;
                self.opt_byte(cc);
            }
            (Kind::Param, _) => {
                let art = has(type_flags(plan, n), DiFlags::ARTIFICIAL);
                self.abbrev(code(fam::FORMAL_PARAMETER, b(art) << 3, false));
                self.ref_to(plan, n, n)?;
            }
            (Kind::Unspecified, _) => self.abbrev(code(fam::UNSPECIFIED_PARAMETERS, 0, false)),
            (Kind::Method, DiNode::Subprogram(sp)) => {
                let a = method_attrs(plan, die);
                let (family, mask) =
                    method_family(&a).ok_or(unsupported(n, "a member function declaration"))?;
                self.abbrev(code(family, mask, children));
                self.opt_name(sp.linkage_name)?;
                self.strx_if(family, sp.name)?;
                let decl = Self::decl_of(plan, n, sp.file, sp.line)?;
                self.decl(decl);
                if a.ty
                    && let Some(DiNode::SubroutineType(s)) = sp.ty.map(|t| plan.debug.node(t))
                {
                    self.ref_to(plan, n, s.types[0])?;
                }
                if a.virt {
                    self.info.push((sp.sp_flags.0 & 3) as u8);
                    let index = u64::from(sp.virtual_index);
                    write_uleb(&mut self.info, 1 + uleb_len(index) as u64);
                    self.info.push(DW_OP_CONSTU);
                    write_uleb(&mut self.info, index);
                }
                self.opt_byte(access(sp.flags));
                if a.cont {
                    self.ref4(plan.aux_of(die));
                }
            }
            _ => unreachable!("a DIE's kind matches its node's: the plan made both"),
        }
        Ok(())
    }

    /// A member function's name: always in the reserved families, when
    /// set in the general ones (whose name is option bit 1).
    fn strx_if(&mut self, family: usize, name: &'a [u8]) -> Result<(), Error> {
        if matches!(family, fam::METHOD | fam::METHOD_QUALIFIED) {
            self.opt_name(name)
        } else {
            self.strx(name)
        }
    }

    /// A member or base (`constructMemberDIE`).
    fn member(
        &mut self,
        plan: &UnitPlan<'a>,
        n: DiId,
        m: &hod_ir::DiDerivedType<'a>,
    ) -> Result<(), Error> {
        let b = u32::from;
        let acc = access(m.flags);
        let virt = has(m.flags, DiFlags::VIRTUAL);
        if m.tag == tag::INHERITANCE {
            if virt {
                self.abbrev(code(fam::INHERITANCE_VIRTUAL, b(acc != 0), false));
                self.opt_ref(plan, n, m.base_type)?;
                let mut expr = [0u8; 16];
                let mut len = 0;
                for op in [DW_OP_DUP, DW_OP_DEREF, DW_OP_CONSTU] {
                    expr[len] = op;
                    len += 1;
                }
                let mut v = m.offset;
                loop {
                    let byte = (v & 0x7f) as u8;
                    v >>= 7;
                    expr[len] = if v == 0 { byte } else { byte | 0x80 };
                    len += 1;
                    if v == 0 {
                        break;
                    }
                }
                for op in [DW_OP_MINUS, DW_OP_DEREF, DW_OP_PLUS] {
                    expr[len] = op;
                    len += 1;
                }
                write_uleb(&mut self.info, len as u64);
                self.info.extend_from_slice(&expr[..len]);
                self.opt_byte(acc);
                self.info.push(DW_VIRTUALITY_VIRTUAL);
            } else {
                self.abbrev(code(fam::INHERITANCE, b(acc != 0), false));
                self.opt_ref(plan, n, m.base_type)?;
                write_uleb(&mut self.info, m.offset / 8);
                self.opt_byte(acc);
            }
            return Ok(());
        }
        let bitfield = has(m.flags, DiFlags::BIT_FIELD);
        let art = has(m.flags, DiFlags::ARTIFICIAL);
        let decl = Self::decl_of(plan, n, m.file, m.line)?;
        let name = !m.name.is_empty();
        if !bitfield && m.align == 0 && !virt {
            let mask = b(name) | b(decl.is_some()) << 1 | 1 << 2 | b(art) << 3 | b(acc != 0) << 4;
            self.abbrev(code(fam::MEMBER, mask, false));
            self.opt_name(m.name)?;
            self.opt_ref(plan, n, m.base_type)?;
            self.decl(decl);
            write_uleb(&mut self.info, m.offset / 8);
            self.opt_byte(acc);
            return Ok(());
        }
        let mask = b(name)
            | b(decl.is_some()) << 1
            | b(bitfield) << 2
            | b(!bitfield && m.align != 0) << 3
            | b(!bitfield) << 4
            | b(acc != 0) << 5
            | b(virt) << 6
            | b(art) << 7;
        self.abbrev(code(fam::MEMBER_ANY, mask, false));
        self.opt_name(m.name)?;
        self.opt_ref(plan, n, m.base_type)?;
        self.decl(decl);
        if bitfield {
            write_uleb(&mut self.info, m.size);
            write_uleb(&mut self.info, m.offset);
        } else {
            self.opt_uleb(u64::from(m.align / 8));
            write_uleb(&mut self.info, m.offset / 8);
        }
        self.opt_byte(acc);
        if virt {
            self.info.push(DW_VIRTUALITY_VIRTUAL);
        }
        Ok(())
    }
}
