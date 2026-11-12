//! How code reaches a symbol: `spec/codegen.md` §7.4.
//!
//! The answer depends only on the module (its `PIE Level`, its code
//! model), the relocation model and the symbol (linkage, visibility,
//! `dso_local`, thread-local model). It follows LLVM 19 and 22:
//! `TargetMachine::shouldAssumeDSOLocal` (on ELF exactly the symbol's
//! `dso_local`, which `hod_ir` stores as LLVM implies it),
//! `TargetMachine::getTLSModel`, and the targets' `ClassifyGlobalReference`
//! rules on top. `tests/sym_access.rs` checks every answer against the
//! relocations `llc` emits over a grid of symbols and options.

use hod_ir::Visibility;
use hod_ir::{Arch, CodeModel, ComdatKind, Linkage, Module, PicLevel, SymId, SymbolKind, TlsModel};

use crate::options::Options;

/// How code takes the address of a symbol (§7.4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Access {
    /// PC-relative or absolute, as the code model says: the symbol is
    /// known to be in the output the code is linked into.
    Direct,
    /// Through a GOT entry: the symbol may be preempted or be undefined
    /// at run time.
    Got,
    /// A thread-local symbol, by this model.
    Tls(TlsAccess),
}

/// A thread-local access model, as `TargetMachine::getTLSModel` chooses
/// it. AArch64 lowers `LocalDynamic` as `GeneralDynamic` (TLS
/// descriptors), as `llc` does by default.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum TlsAccess {
    /// Through the dynamic TLS resolver, for any symbol.
    GeneralDynamic,
    /// Through the module's TLS block: a symbol of this output.
    LocalDynamic,
    /// Through a GOT entry holding the offset from the thread pointer.
    InitialExec,
    /// An offset from the thread pointer fixed at link time.
    LocalExec,
}

/// How code calls a function (§7.4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CallAccess {
    /// A direct branch: the callee is in this output.
    Direct,
    /// A branch the linker may route through a PLT entry.
    Plt,
}

/// How code addresses `sym` (§7.4).
pub fn sym_access(module: &Module, options: &Options, sym: SymId) -> Access {
    let symbol = module.symbol(sym);
    let tls = match symbol.kind() {
        SymbolKind::Variable { tls, .. } | SymbolKind::Alias { tls, .. } => tls,
        _ => TlsModel::None,
    };
    if tls != TlsModel::None {
        return Access::Tls(tls_access(module, *options, symbol.dso_local(), tls));
    }
    if !symbol.dso_local() {
        return Access::Got;
    }
    // AArch64's small and tiny code models reach a direct address with
    // `adrp` or `adr`, which cannot produce 0 for an undefined weak
    // symbol (`AArch64Subtarget::ClassifyGlobalReference`).
    let small = matches!(options.code_model(module), CodeModel::Small | CodeModel::Tiny);
    if module.target().arch() == Arch::Aarch64 && small && symbol.linkage() == Linkage::ExternWeak {
        return Access::Got;
    }
    Access::Direct
}

/// How code calls `sym`, a function or an alias or IFUNC of one (§7.4).
pub fn call_access(module: &Module, sym: SymId) -> CallAccess {
    if module.symbol(sym).dso_local() { CallAccess::Direct } else { CallAccess::Plt }
}

/// `TargetMachine::getTLSModel`: the model the output kind implies,
/// unless the symbol asks for a more specific one.
fn tls_access(module: &Module, options: Options, dso_local: bool, asked: TlsModel) -> TlsAccess {
    let shared_library = options.reloc_model.is_pic() && module.props.pie == PicLevel::None;
    let implied = match (shared_library, dso_local) {
        (true, true) => TlsAccess::LocalDynamic,
        (true, false) => TlsAccess::GeneralDynamic,
        (false, true) => TlsAccess::LocalExec,
        (false, false) => TlsAccess::InitialExec,
    };
    let asked = match asked {
        TlsModel::None | TlsModel::GeneralDynamic => TlsAccess::GeneralDynamic,
        TlsModel::LocalDynamic => TlsAccess::LocalDynamic,
        TlsModel::InitialExec => TlsAccess::InitialExec,
        TlsModel::LocalExec => TlsAccess::LocalExec,
    };
    implied.max(asked)
}

/// Whether code refers to `sym` through a local alias rather than its
/// own symbol (`AsmPrinter::getSymbolPreferLocal`): in a shared object
/// (position-independent, no `PIE Level`), a `dso_local` definition with
/// external linkage and default visibility is still preemptible as far as
/// the linker knows, so a direct reference to its global symbol would be
/// refused. Module emission makes the alias (§9.7).
pub fn prefers_local_alias(module: &Module, options: Options, sym: SymId) -> bool {
    if !options.reloc_model.is_pic() || module.props.pie != PicLevel::None {
        return false;
    }
    let symbol = module.symbol(sym);
    let comdat_dedup =
        symbol.comdat().is_some_and(|c| module.comdat(c).kind != ComdatKind::NoDeduplicate);
    symbol.dso_local()
        && symbol.visibility() == Visibility::Default
        && symbol.linkage() == Linkage::External
        && symbol.is_definition()
        && !matches!(symbol.kind(), SymbolKind::IFunc { .. })
        && !comdat_dedup
}
