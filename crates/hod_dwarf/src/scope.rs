//! The DIEs outside function bodies that WP-059 writes (`spec/dwarf.md`
//! §6.11): global and static-member variables of every shape, template
//! parameters, imported entities, and the declarations a definition's
//! `DW_AT_specification` refers to. This module holds the stable names of
//! what is still refused and the plan's state for them; the work is in
//! `global.rs`, `template.rs`, `import.rs` and `spec.rs`.

use hod_ir::{Arch, DiId, Module, SymId, TlsModel};
use hod_obj::SymbolId;

/// A global's `DIExpression` that is neither empty nor a constant
/// (`DW_OP_constu`/`DW_OP_consts`, `DW_OP_stack_value`).
pub const GLOBAL_EXPRESSION: &str = "debug:global-expression";
/// A global with several `DIGlobalVariableExpression`s.
pub const GLOBAL_EXPRESSIONS: &str = "debug:global-expressions";
/// A global whose shape `llc` does not write: a static member's
/// definition that is a declaration, a nameless global with a template
/// parameter, a declaration or a constant, a global without a type or
/// with a line and no file.
pub const GLOBAL_SHAPE: &str = "debug:global-shape";
/// A global whose `scope:` is neither the unit, a file nor a namespace.
pub const GLOBAL_SCOPE: &str = "debug:global-scope";
/// A static member's definition whose `declaration:` is not a static
/// member of a class.
pub const STATIC_MEMBER_DECLARATION: &str = "debug:static-member-declaration";
/// A thread-local variable's location on a target whose `llc` writes one
/// (x86-64: `DW_OP_const8u` + `DW_OP_GNU_push_tls_address`); AArch64's
/// `llc` writes none, nor does hod.
pub const TLS_LOCATION: &str = "debug:tls-location";
/// A template parameter of a tag other than the four `llc` writes, a pack
/// or template template parameter with a default, or a value of the wrong
/// kind for its tag.
pub const TEMPLATE_PARAMETER: &str = "debug:template-parameter";
/// A template value parameter whose integer is wider than 64 bits
/// (`llc` writes a block).
pub const TEMPLATE_VALUE: &str = "debug:template-value";
/// A template value parameter that is the address of a symbol this
/// object does not define (its relocation would need the symbol named).
pub const TEMPLATE_ADDRESS: &str = "debug:template-address";
/// An imported entity of a tag other than `imported_module` and
/// `imported_declaration`, with `elements:`, or an `imported_module` with
/// a name.
pub const IMPORT: &str = "debug:import";
/// An imported entity whose `scope:` is not the unit, a file or a
/// namespace (a unit's), or not its function (a function's).
pub const IMPORT_SCOPE: &str = "debug:import-scope";
/// An imported entity whose entity `llc` would make a DIE for that hod
/// does not: a module, another imported entity, a function's definition
/// without code in the unit, a function-local variable.
pub const IMPORT_ENTITY: &str = "debug:import-entity";
/// A function declaration (the target of a definition's
/// `declaration:`, or an imported entity) scoped outside a class, the
/// unit, a file or a namespace, or virtual outside a class.
pub const DECLARATION_SCOPE: &str = "debug:declaration-scope";

/// The shape of a variable DIE that WP-059 writes (sorted by DIE).
#[derive(Clone, Copy, Debug)]
pub(crate) struct VarShape {
    /// The static member declaration's DIE, or `NONE`.
    pub(crate) spec: u32,
    /// `DW_AT_type` with a specification: the definition's type differs
    /// from the declaration's.
    pub(crate) spec_type: bool,
    /// `DW_AT_const_value`: the bits and whether they are signed.
    pub(crate) constant: Option<(u64, bool)>,
    /// A thread-local variable on AArch64: no `DW_AT_location`.
    pub(crate) tls: bool,
}

/// The plan's state for WP-059's DIEs.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScopeDies {
    /// The module's target, when the plan has the module.
    pub(crate) arch: Option<Arch>,
    /// The `DIGlobalVariableExpression`s whose symbol is thread-local,
    /// sorted by id.
    pub(crate) tls: Vec<u32>,
    /// `(DIE, shape)` of the variables written here, in DIE order.
    pub(crate) vars: Vec<(u32, VarShape)>,
    /// The symbols template value parameters take the address of, sorted
    /// and unique, with their placement once the code generator gave it.
    pub(crate) syms: Vec<(SymId, Option<(SymbolId, i64)>)>,
    /// The template parameter DIEs that take a `.debug_addr` entry.
    pub(crate) param_addrs: u32,
}

impl ScopeDies {
    /// The state of a plan of `module`'s tables (or of tables alone).
    pub(crate) fn new(module: Option<&Module>) -> Self {
        let Some(m) = module else { return Self::default() };
        let mut tls: Vec<u32> = m
            .global_exprs()
            .iter()
            .filter(|&&(sym, _)| m.symbol(sym).tls() != TlsModel::None)
            .map(|&(_, expr)| expr.raw())
            .collect();
        tls.sort_unstable();
        tls.dedup();
        Self { arch: Some(m.target().arch()), tls, ..Self::default() }
    }

    pub(crate) fn is_tls(&self, expr: DiId) -> bool {
        self.tls.binary_search(&expr.raw()).is_ok()
    }

    pub(crate) fn var(&self, die: u32) -> Option<VarShape> {
        let i = self.vars.binary_search_by_key(&die, |v| v.0).ok()?;
        Some(self.vars[i].1)
    }

    /// Records a symbol a template parameter takes the address of.
    pub(crate) fn want_symbol(&mut self, sym: SymId) {
        if let Err(at) = self.syms.binary_search_by_key(&sym.index(), |s| s.0.index()) {
            self.syms.insert(at, (sym, None));
        }
    }

    pub(crate) fn placed(&self, sym: SymId) -> Option<(SymbolId, i64)> {
        let i = self.syms.binary_search_by_key(&sym.index(), |s| s.0.index()).ok()?;
        self.syms[i].1
    }
}
