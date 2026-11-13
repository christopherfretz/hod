//! The inline-assembly table (`spec/ir.md` §2): the callee of an
//! `asm` call, interned per module as an [`AsmId`].

use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::{AsmId, TypeId};
use crate::intern::{Hasher64, InternTable, Layered, Layering, NoLayer, Seg, next_id};
use crate::strings::{StrId, StrPool};
use crate::types::{TypeKind, TypeTable};

/// One inline-assembly callee: LLVM's `asm "template", "constraints"`
/// with its flags and the type it is called at. Both what a producer
/// passes to `Module::inline_asm` and what `Module::asm` returns.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct InlineAsm<'a> {
    /// The assembly text, with LLVM's `$0`, `${1:w}` operand references.
    pub template: &'a [u8],
    /// The comma-separated constraint string.
    pub constraints: &'a [u8],
    /// An OR of the flag constants.
    pub flags: u8,
    /// The function type of the call.
    pub ty: TypeId,
}

impl InlineAsm<'_> {
    /// `sideeffect`.
    pub const SIDE_EFFECT: u8 = 1 << 0;
    /// `alignstack`.
    pub const ALIGN_STACK: u8 = 1 << 1;
    /// `inteldialect`.
    pub const INTEL_DIALECT: u8 = 1 << 2;
    /// `unwind`: the assembly may unwind.
    pub const UNWIND: u8 = 1 << 3;

    const ALL_FLAGS: u8 = (1 << 4) - 1;

    /// True if every flag in `flags` is set.
    #[inline]
    pub const fn has(&self, flags: u8) -> bool {
        self.flags & flags == flags
    }
}

/// A stored callee: 16 bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Entry {
    template: StrId,
    constraints: StrId,
    ty: TypeId,
    flags: u8,
}

/// The inline-assembly callees of a module.
#[derive(Clone, Debug, Default)]
pub(crate) struct AsmTable<L: Layering = NoLayer> {
    entries: Seg<Entry, L>,
    strings: StrPool<L>,
    interner: InternTable<L>,
}

impl AsmTable {
    pub(crate) fn seed(&self) -> AsmTable<Layered> {
        AsmTable {
            entries: self.entries.seed(),
            strings: self.strings.seed(),
            interner: self.interner.seed(),
        }
    }
}

impl AsmTable<Layered> {
    pub(crate) fn fork(&self) -> AsmTable<Layered> {
        AsmTable {
            entries: self.entries.fork(),
            strings: self.strings.fork(),
            interner: self.interner.fork(),
        }
    }

    pub(crate) fn local_is_empty(&self) -> bool {
        self.entries.local_is_empty()
    }

    /// The callees from `from` on, as owned values (their strings may be
    /// older than they are: the pool shares equal strings).
    pub(crate) fn owned_since(&self, from: usize) -> Vec<OwnedAsm> {
        (from..self.entries.len())
            .map(|id| {
                let asm = self.get(AsmId::new(id as u32));
                OwnedAsm {
                    template: asm.template.into(),
                    constraints: asm.constraints.into(),
                    flags: asm.flags,
                    ty: asm.ty,
                }
            })
            .collect()
    }
}

impl<L: Layering> AsmTable<L> {
    /// The id of `asm`, added if new.
    pub(crate) fn intern(&mut self, types: &TypeTable<L>, asm: &InlineAsm<'_>) -> IrResult<AsmId> {
        if !matches!(types.kind(asm.ty), TypeKind::Function { .. }) {
            return err(
                IrErrorKind::InvalidType,
                format!("inline assembly is called at a function type, not {:?}", asm.ty),
            );
        }
        if asm.flags & !InlineAsm::ALL_FLAGS != 0 {
            return err(
                IrErrorKind::InvalidOperand,
                format!("{:#x} is not a set of inline-assembly flags", asm.flags),
            );
        }
        let entry = Entry {
            template: self.strings.intern(asm.template)?,
            constraints: self.strings.intern(asm.constraints)?,
            ty: asm.ty,
            flags: asm.flags,
        };
        let mut h = Hasher64::new(u64::from(entry.flags));
        h.pair(entry.template.raw(), entry.constraints.raw());
        h.word(u64::from(entry.ty.raw()));
        let hash = h.finish();
        if let Some(found) = self.interner.find(hash, |id| self.entries[id as usize] == entry) {
            return Ok(AsmId::new(found));
        }
        let id = next_id(self.entries.len(), "inline assembly")?;
        self.entries.push(entry);
        self.interner.insert(hash, id);
        Ok(AsmId::new(id))
    }

    /// The callee `id`.
    pub(crate) fn get(&self, id: AsmId) -> InlineAsm<'_> {
        let entry = &self.entries[id.index()];
        InlineAsm {
            template: self.strings.get(entry.template),
            constraints: self.strings.get(entry.constraints),
            flags: entry.flags,
            ty: entry.ty,
        }
    }

    /// Number of distinct callees.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// An inline-assembly callee that owns its strings.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct OwnedAsm {
    pub(crate) template: Box<[u8]>,
    pub(crate) constraints: Box<[u8]>,
    pub(crate) flags: u8,
    pub(crate) ty: TypeId,
}

impl OwnedAsm {
    pub(crate) fn view(&self, ty: TypeId) -> InlineAsm<'_> {
        InlineAsm {
            template: &self.template,
            constraints: &self.constraints,
            flags: self.flags,
            ty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::TargetSpec;

    /// §2: an inline-assembly callee is its template, constraints, flags
    /// and function type, and equal callees are one id.
    #[test]
    fn inline_asm_is_interned() {
        assert_eq!(size_of::<Entry>(), 16);
        let mut types = TypeTable::new(TargetSpec::x86_64_linux_gnu().data_layout());
        let void_fn = types.function(TypeId::VOID, &[], false).unwrap();
        let int_fn = types.function(TypeId::I32, &[TypeId::I32], false).unwrap();
        let mut table = AsmTable::default();
        let nop = InlineAsm { template: b"nop", constraints: b"", flags: 0, ty: void_fn };
        let a = table.intern(&types, &nop).unwrap();
        assert_eq!(table.intern(&types, &nop).unwrap(), a);
        let variants = [
            InlineAsm { template: b"pause", ..nop },
            InlineAsm { constraints: b"~{memory}", ..nop },
            InlineAsm { flags: InlineAsm::SIDE_EFFECT, ..nop },
            InlineAsm { ty: int_fn, ..nop },
            InlineAsm {
                template: b"bswap $0",
                constraints: b"=r,0,~{dirflag},~{fpsr},~{flags}",
                flags: InlineAsm::ALIGN_STACK | InlineAsm::INTEL_DIALECT | InlineAsm::UNWIND,
                ty: int_fn,
            },
        ];
        let mut ids = vec![a];
        for variant in &variants {
            let id = table.intern(&types, variant).unwrap();
            assert!(!ids.contains(&id), "{variant:?} is a callee of its own");
            assert_eq!(table.intern(&types, variant).unwrap(), id);
            assert_eq!(table.get(id), *variant);
            ids.push(id);
        }
        assert_eq!(table.len(), 6);
        assert_eq!(table.get(a), nop);
        let last = table.get(ids[5]);
        assert!(last.has(InlineAsm::ALIGN_STACK | InlineAsm::UNWIND));
        assert!(!last.has(InlineAsm::SIDE_EFFECT));
    }

    /// §2: a callee that is not called at a function type, or has an
    /// unknown flag, is an error.
    #[test]
    fn inline_asm_errors() {
        let mut types = TypeTable::new(TargetSpec::x86_64_linux_gnu().data_layout());
        let void_fn = types.function(TypeId::VOID, &[], false).unwrap();
        let mut table = AsmTable::default();
        let nop = InlineAsm { template: b"nop", constraints: b"", flags: 0, ty: void_fn };
        let not_fn = InlineAsm { ty: TypeId::I32, ..nop };
        assert_eq!(table.intern(&types, &not_fn).unwrap_err().kind, IrErrorKind::InvalidType);
        let bad_flag = InlineAsm { flags: 1 << 4, ..nop };
        assert_eq!(table.intern(&types, &bad_flag).unwrap_err().kind, IrErrorKind::InvalidOperand);
        assert_eq!(table.len(), 0);
    }
}
