//! Module-level assembly made only of symbol directives (`spec/ir.md`
//! §2.3, `spec/codegen.md` §9.7.1): WP-075. `hod_ir` reads the text and
//! checks it against the module; this writes the effects into the
//! object, after every module symbol is defined: an undefined symbol
//! for a name the module does not have, and the binding, visibility and
//! type a directive gives a declaration or a definition. Nothing else of
//! the object changes: a directive makes no relocation.

use hod_ir::{AsmBinding, AsmSymbolType, Module};
use hod_obj::{Binding, Symbol, SymbolKind};

use super::{ModuleEmitter, obj_err, unsupported, visibility};
use crate::error::{CgError, CgResult};

/// Refuses module-level assembly an assembler must read
/// ([`unsupported::MODULE_ASM`]).
///
/// # Errors
/// [`unsupported::MODULE_ASM`].
pub(super) fn check(module: &Module) -> CgResult<()> {
    if module.module_asm().is_empty() {
        return Ok(());
    }
    let mut effects = Vec::new();
    hod_ir::module_asm_effects(module, &mut effects)
        .map_err(|_| CgError::unsupported(unsupported::MODULE_ASM))
}

impl ModuleEmitter<'_> {
    /// Writes the symbol effects of the module's assembly (`spec/ir.md`
    /// §2.3) into the object. Called once, after `define`.
    ///
    /// # Errors
    /// [`unsupported::MODULE_ASM`] for text [`check`] refuses; an
    /// internal error if the object refuses a name.
    pub(super) fn module_asm_symbols(&mut self) -> CgResult<()> {
        let module = self.module;
        if module.module_asm().is_empty() {
            return Ok(());
        }
        let mut effects = Vec::new();
        hod_ir::module_asm_effects(module, &mut effects)
            .map_err(|_| CgError::unsupported(unsupported::MODULE_ASM))?;
        for effect in &effects {
            let id = if let Some(sym) = effect.sym {
                if effect.retain {
                    self.name_symbol(sym)?;
                }
                Self::obj_sym(sym)
            } else {
                let name = self.obj.add_string(effect.name).map_err(obj_err)?;
                self.obj.add_symbol(Symbol::undefined(name))
            };
            if effect.retain {
                self.obj.retain_symbol(id);
            }
            let target = self.obj.symbol_mut(id);
            match effect.binding {
                Some(AsmBinding::Global) => target.binding = Binding::Global,
                Some(AsmBinding::Weak) => target.binding = Binding::Weak,
                None => {}
            }
            if let Some(v) = effect.visibility {
                target.visibility = visibility(v);
            }
            match effect.ty {
                Some(AsmSymbolType::Function) => target.kind = SymbolKind::Func,
                Some(AsmSymbolType::Object) => target.kind = SymbolKind::Object,
                Some(AsmSymbolType::NoType) => target.kind = SymbolKind::NoType,
                None => {}
            }
        }
        Ok(())
    }
}
