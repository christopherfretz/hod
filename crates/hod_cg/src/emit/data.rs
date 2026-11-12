//! What a variable's initializer says about its section (`spec/codegen.md`
//! §9.7): LLVM's `isSuitableForBSS`, `IsNullTerminatedString` and
//! `Constant::getRelocationInfo`, over `hod_ir` constants and fixups.

use hod_ir::{Const, ConstId, FixupTarget, Module, TypeId, TypeKind};

/// Whether `c` is all zero or all undefined, as LLVM sees it after its
/// constant folding: `isNullValue` or `UndefValue`. An aggregate that
/// mixes zero and undefined elements is neither.
pub(crate) fn is_bss_suitable(module: &Module, c: ConstId) -> bool {
    zero_or_undef(module, c).is_some()
}

/// `Some(true)` for all undefined, `Some(false)` for all zero.
fn zero_or_undef(module: &Module, c: ConstId) -> Option<bool> {
    let consts = &module.consts;
    match consts.get(c) {
        Const::Undef(_) | Const::Poison(_) => Some(true),
        Const::Aggregate { elems, .. } => {
            let mut all = None;
            for &elem in elems {
                let this = zero_or_undef(module, elem)?;
                if all.is_some_and(|all| all != this) {
                    return None;
                }
                all = Some(this);
            }
            Some(all.unwrap_or(false))
        }
        Const::Splat { elem, .. } => zero_or_undef(module, elem),
        Const::Bytes { bytes, .. } => bytes.iter().all(|&b| b == 0).then_some(false),
        _ => consts.is_zero(c).then_some(false),
    }
}

/// The character size of `c` when it is a NUL-terminated string with no
/// other NUL: an array of `i8`, `i16` or `i32` (`IsNullTerminatedString`).
pub(crate) fn cstring_char_size(module: &Module, ty: TypeId, c: ConstId) -> Option<u8> {
    let types = &module.types;
    let TypeKind::Array { elem, len } = types.kind(ty) else { return None };
    let size = match types.kind(elem) {
        TypeKind::Int(8) => 1,
        TypeKind::Int(16) => 2,
        TypeKind::Int(32) => 4,
        _ => return None,
    };
    let terminated = |values: &mut dyn Iterator<Item = Option<u64>>| {
        let mut count = 0u64;
        let mut last_zero = false;
        for value in values {
            let Some(value) = value else { return false };
            if last_zero {
                return false;
            }
            last_zero = value == 0;
            count += 1;
        }
        count == len && last_zero
    };
    let ok = match module.consts.get(c) {
        Const::Bytes { bytes, .. } => terminated(&mut bytes.iter().map(|&b| Some(u64::from(b)))),
        Const::Aggregate { elems, .. } => {
            terminated(&mut elems.iter().map(|&e| module.consts.int_value(e)))
        }
        // `[1 x i8] zeroinitializer`.
        Const::Zero(_) => len == 1,
        _ => false,
    };
    ok.then_some(size)
}

/// `Constant::PossibleRelocationsTy`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Relocs {
    /// The image is a constant.
    None,
    /// Only differences of symbols the static linker resolves.
    Local,
    /// Something the dynamic linker may have to fix up.
    Global,
}

/// The relocations an initializer with these fixups needs.
pub(crate) fn reloc_info(module: &Module, fixups: &[hod_ir::Fixup]) -> Relocs {
    let local = |sym| module.symbol(sym).dso_local();
    fixups.iter().fold(Relocs::None, |most, fixup| {
        let this = match fixup.target {
            FixupTarget::Diff { a, b } if local(a) && local(b) => Relocs::Local,
            FixupTarget::PltDiff { b, .. } if local(b) => Relocs::Local,
            _ => Relocs::Global,
        };
        most.max(this)
    })
}
