//! [`Support`]: the support matrix as a value (`spec/ir.md` §8.2).

use std::fmt;

use super::matrix::{
    ROW_COUNT, SUPPORT_ROWS, SupportStatus, TypeClass, TypeClasses, at, row_index,
};
use crate::attrs::FnAttrs;
use crate::inst::Opcode;
use crate::target::Arch;

/// [`Support::short`]: the short way decides the opcode (its row is
/// supported, and it has no row or rule of its own beyond its subjects).
pub(crate) const SHORT_ON: u32 = 1 << 16;
/// The opcode is a `load` or a `store`: a volatile or atomic one goes the
/// long way.
pub(crate) const SHORT_MEM: u32 = 1 << 17;
/// The opcode is `call`.
pub(crate) const SHORT_CALL: u32 = 1 << 18;
/// The operands are a list ([`Opcode::inline_values`] is `None`).
pub(crate) const SHORT_LIST: u32 = 1 << 19;
/// Where the number of inline operands is.
pub(crate) const SHORT_COUNT_SHIFT: u32 = 20;

/// What a code generator implements, for one target: the support matrix
/// with some rows switched on (§8.2).
///
/// [`Support::shipped`] is the matrix as checked in, and what
/// [`classify`](crate::classify()) uses. [`Support::everything_implemented`]
/// is the matrix with every `not-implemented` row switched on: what is
/// still refused under it is refused by design. [`Support::with`] and
/// [`Support::without`] switch single rows, for tests and for a code
/// generator that wants to try a row before the matrix says so.
#[derive(Clone, PartialEq, Eq)]
pub struct Support {
    arch: Arch,
    /// Per row, the classes it is supported for; 0 for a row that is not
    /// supported. The last entry is not a row: it has every class.
    pub(crate) mask: [u16; ROW_COUNT + 1],
    /// The classes whose `type:` row is supported.
    pub(crate) types: u16,
    /// The `FnAttrs` flags whose row is not supported.
    pub(crate) fn_flags_off: u32,
    /// Per opcode, what the short way of the body pass needs (§8.11): the
    /// classes its row can be given (`mask & types`) in the low 16 bits,
    /// and the `SHORT_*` bits.
    pub(crate) short: [u32; Opcode::ALL.len()],
}

static SHIPPED_AARCH64: Support = Support::build(Arch::Aarch64, false);
static SHIPPED_X86_64: Support = Support::build(Arch::X86_64, false);

impl Support {
    const fn build(arch: Arch, everything: bool) -> Support {
        let mut mask = [0; ROW_COUNT + 1];
        let mut index = 0;
        while index < ROW_COUNT {
            let cell = SUPPORT_ROWS[index].cell(arch);
            mask[index] = match cell.status {
                SupportStatus::Rejected(_) => 0,
                _ if everything => TypeClasses::ALL.bits(),
                SupportStatus::Supported => cell.classes.bits(),
                SupportStatus::NotImplemented(_) => 0,
            };
            index += 1;
        }
        mask[at::ANY] = TypeClasses::ALL.bits();
        Support { arch, mask, types: 0, fn_flags_off: 0, short: [0; Opcode::ALL.len()] }.derived()
    }

    /// Fills the fields computed from `mask`.
    const fn derived(mut self) -> Support {
        self.types = 0;
        let mut class = 0;
        while class < TypeClass::ALL.len() {
            if self.mask[at::TYPE + class] != 0 {
                self.types |= 1 << class;
            }
            class += 1;
        }
        self.fn_flags_off = 0;
        let mut flag = 0;
        while flag < FnAttrs::FLAG_NAMES.len() {
            if self.mask[at::ATTR_FLAG + flag] == 0 {
                self.fn_flags_off |= 1 << flag;
            }
            flag += 1;
        }
        let mut index = 0;
        while index < Opcode::ALL.len() {
            let op = Opcode::ALL[index];
            let have = self.mask[at::INST + op as usize];
            let short = match op {
                _ if have == 0 => 0,
                // These have rows and rules of their own.
                Opcode::CmpXchg
                | Opcode::AtomicRmw
                | Opcode::LandingPad
                | Opcode::Switch
                | Opcode::Invoke
                | Opcode::CallBr => 0,
                Opcode::Call => SHORT_ON | SHORT_CALL,
                Opcode::Load | Opcode::Store => SHORT_ON | SHORT_MEM,
                _ => SHORT_ON,
            };
            let operands = match op.inline_values() {
                Some(count) => (count as u32) << SHORT_COUNT_SHIFT,
                None => SHORT_LIST,
            };
            self.short[op as usize] =
                if short == 0 { 0 } else { short | operands | (have & self.types) as u32 };
            index += 1;
        }
        self
    }

    /// The matrix as checked in, for `arch`: what hod's code generator
    /// implements today.
    pub fn shipped(arch: Arch) -> &'static Support {
        match arch {
            Arch::Aarch64 => &SHIPPED_AARCH64,
            Arch::X86_64 => &SHIPPED_X86_64,
        }
    }

    /// The matrix with every row that is not `Rejected` supported for
    /// every class: only what is refused by design is refused.
    pub const fn everything_implemented(arch: Arch) -> Support {
        Support::build(arch, true)
    }

    /// The architecture this is the matrix of.
    pub const fn arch(&self) -> Arch {
        self.arch
    }

    /// The row and the classes `name` stands for: a row's name is the
    /// row with every class, `row/class` is one class of it.
    fn parse(name: &str) -> Option<(usize, TypeClasses)> {
        if let Some(row) = row_index(name) {
            return Some((row, TypeClasses::ALL));
        }
        let (row, class) = name.rsplit_once('/')?;
        Some((row_index(row)?, TypeClasses::of(&[TypeClass::named(class)?])))
    }

    fn rejected(&self, row: usize) -> bool {
        matches!(SUPPORT_ROWS[row].cell(self.arch).status, SupportStatus::Rejected(_))
    }

    /// The same with the construct `name` supported: a row name (for
    /// every class), or a pair `row/class` (§8.3) to add one class.
    /// `None` if `name` is neither, or if the row is `Rejected` for this
    /// target: no `Support` accepts those.
    #[must_use]
    pub fn with(self, name: &str) -> Option<Support> {
        let (row, classes) = Support::parse(name)?;
        self.with_classes(SUPPORT_ROWS[row].name, classes)
    }

    /// The same with row `row` supported for `classes` as well as for
    /// those it already has. `None` if there is no such row or it is
    /// `Rejected` for this target.
    #[must_use]
    pub fn with_classes(mut self, row: &str, classes: TypeClasses) -> Option<Support> {
        let row = row_index(row)?;
        if self.rejected(row) {
            return None;
        }
        self.mask[row] |= classes.bits();
        Some(self.derived())
    }

    /// The same with the construct `name` not supported: a row name
    /// switches the row off, a pair `row/class` takes one class away.
    /// `None` if `name` is neither.
    #[must_use]
    pub fn without(mut self, name: &str) -> Option<Support> {
        let (row, classes) = Support::parse(name)?;
        self.mask[row] &= !classes.bits();
        Some(self.derived())
    }

    /// Whether the row called `name`, or the pair `row/class`, is
    /// supported. `None` if `name` is neither.
    pub fn supports(&self, name: &str) -> Option<bool> {
        let (row, classes) = Support::parse(name)?;
        let have = self.mask[row];
        Some(if classes == TypeClasses::ALL { have != 0 } else { have & classes.bits() != 0 })
    }

    /// The classes row `row` is supported for; empty when it is not
    /// supported. `None` if there is no such row.
    pub fn classes(&self, row: &str) -> Option<TypeClasses> {
        Some(TypeClasses::from_bits(self.mask[row_index(row)?]))
    }
}

impl fmt::Debug for Support {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let supported = self.mask[..ROW_COUNT].iter().filter(|&&classes| classes != 0).count();
        f.debug_struct("Support")
            .field("arch", &self.arch)
            .field("supported_rows", &supported)
            .field("rows", &ROW_COUNT)
            .finish_non_exhaustive()
    }
}
