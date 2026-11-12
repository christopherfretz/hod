//! Module properties and LLVM module flags (`spec/ir.md` §2.1).
//!
//! [`ModuleProps`] holds the module-wide settings that change the object
//! file. A producer either fills it in directly or passes each LLVM module
//! flag to `Module::set_module_flag`, which maps the flags hod knows onto
//! the properties and keeps every other flag by name, marked ignorable or
//! not, for `classify` (§8) to judge.

use crate::attrs::UwTable;
use crate::error::{IrErrorKind, IrResult, err};
use crate::strings::{StrId, StrPool};

/// The `PIC Level` and `PIE Level` module flags: whether the code is
/// position independent, and with which GOT size model (`-fpic` is small,
/// `-fPIC` big; the two differ on a few targets, not on AArch64 or x86-64).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
#[repr(u8)]
pub enum PicLevel {
    /// Not position independent (flag absent, or 0).
    #[default]
    None,
    /// Level 1.
    Small,
    /// Level 2.
    Big,
}

/// The `Code Model` module flag, with LLVM's numbers.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum CodeModel {
    /// 0.
    Tiny,
    /// 1.
    Small,
    /// 2.
    Kernel,
    /// 3.
    Medium,
    /// 4.
    Large,
}

/// The `frame-pointer` module flag, with LLVM's numbers. LLVM 19 has 0 to
/// 3; 4 is new in LLVM 22, where clang uses it as the AArch64 default.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
#[repr(u8)]
pub enum FramePointer {
    /// 0, or flag absent: no function needs a frame pointer.
    #[default]
    None,
    /// 1: functions that call need one.
    NonLeaf,
    /// 2: every function needs one.
    All,
    /// 3: none is needed, but the register is not used for anything else.
    Reserved,
    /// 4: as `NonLeaf`, and leaf functions may use the register.
    NonLeafNoReserve,
}

/// The branch-protection settings of a module: a set of flag bits.
///
/// These decide the object's `.note.gnu.property`; whether one function
/// is protected is that function's string attributes (§5.7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct BranchProtection(u8);

impl BranchProtection {
    /// AArch64 BTI: module flag `branch-target-enforcement`.
    pub const BTI: u8 = 1 << 0;
    /// AArch64 guarded control stack: `guarded-control-stack`.
    pub const GCS: u8 = 1 << 1;
    /// AArch64 return-address signing: `sign-return-address`.
    pub const PAC_RET: u8 = 1 << 2;
    /// Signing scope "all functions", not only non-leaf ones:
    /// `sign-return-address-all`.
    pub const PAC_RET_ALL: u8 = 1 << 3;
    /// Signing with the B key: `sign-return-address-with-bkey`.
    pub const PAC_B_KEY: u8 = 1 << 4;
    /// Signing with the PC as a second modifier:
    /// `branch-protection-pauth-lr`.
    pub const PAC_PAUTH_LR: u8 = 1 << 5;
    /// x86 CET indirect-branch tracking: `cf-protection-branch`.
    pub const CET_IBT: u8 = 1 << 6;
    /// x86 CET shadow stack: `cf-protection-return`.
    pub const CET_SHSTK: u8 = 1 << 7;

    /// No protection.
    pub const NONE: BranchProtection = BranchProtection(0);

    /// The set with the bits of `bits` (an OR of the constants).
    pub const fn from_bits(bits: u8) -> BranchProtection {
        BranchProtection(bits)
    }

    /// The bits, an OR of the constants.
    #[inline]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// True if every bit in `bits` is set.
    #[inline]
    pub const fn has(self, bits: u8) -> bool {
        self.0 & bits == bits
    }

    /// Sets or clears the bits in `bits`.
    pub const fn set(&mut self, bits: u8, on: bool) {
        if on {
            self.0 |= bits;
        } else {
            self.0 &= !bits;
        }
    }
}

/// The module-wide settings that change the generated object (§2.1).
/// The default is what a module with no module flags has.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ModuleProps {
    /// `PIC Level`.
    pub pic: PicLevel,
    /// `PIE Level`: the code is position independent and will be linked
    /// into an executable, so definitions cannot be preempted.
    pub pie: PicLevel,
    /// `Code Model`, or `None` for the target's default.
    pub code_model: Option<CodeModel>,
    /// `Large Data Threshold`: with the medium code model on x86-64, data
    /// larger than this many bytes is "large".
    pub large_data_threshold: Option<u64>,
    /// `uwtable`: the default unwind-table kind.
    pub uwtable: UwTable,
    /// `frame-pointer`: the default frame-pointer policy.
    pub frame_pointer: FramePointer,
    /// The branch-protection flags.
    pub branch_protection: BranchProtection,
    /// `Dwarf Version`, or `None` when the flag is absent.
    pub dwarf_version: Option<u16>,
    /// `Debug Info Version`, or `None` when the flag is absent. LLVM
    /// drops the debug information of a module whose version is not 3.
    pub debug_info_version: Option<u32>,
}

/// A module flag's merge behavior, with LLVM's numbers. It says what a
/// linker of IR modules does with two flags of one name; for one module
/// only `Require` differs: it states a condition and sets nothing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum FlagBehavior {
    /// 1.
    Error = 1,
    /// 2.
    Warning,
    /// 3.
    Require,
    /// 4.
    Override,
    /// 5.
    Append,
    /// 6.
    AppendUnique,
    /// 7.
    Max,
    /// 8.
    Min,
}

impl FlagBehavior {
    /// The behavior with LLVM's number, 1 to 8.
    pub const fn from_llvm(value: u32) -> Option<FlagBehavior> {
        Some(match value {
            1 => FlagBehavior::Error,
            2 => FlagBehavior::Warning,
            3 => FlagBehavior::Require,
            4 => FlagBehavior::Override,
            5 => FlagBehavior::Append,
            6 => FlagBehavior::AppendUnique,
            7 => FlagBehavior::Max,
            8 => FlagBehavior::Min,
            _ => return None,
        })
    }
}

/// A module flag's value.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FlagValue<'a> {
    /// A constant integer of any width up to 64 bits, zero-extended.
    Int(u64),
    /// A metadata string.
    Str(&'a [u8]),
    /// Anything else: a metadata node, a wider integer, another constant.
    Other,
}

/// What hod does with a module flag name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FlagClass {
    /// It maps onto [`ModuleProps`].
    Known,
    /// It has no effect on the object hod writes for its targets.
    Ignorable,
    /// Neither. `classify` refuses the module as `module-flag:<name>`.
    Unknown,
}

/// The flags that map onto [`ModuleProps`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum Known {
    PicLevel,
    PieLevel,
    CodeModel,
    LargeDataThreshold,
    UwTable,
    FramePointer,
    DwarfVersion,
    DebugInfoVersion,
    Bti,
    Gcs,
    PacRet,
    PacRetAll,
    PacBKey,
    PacPauthLr,
    CetIbt,
    CetShstk,
}

/// The names of the known flags (§2.1).
pub const KNOWN_MODULE_FLAGS: [&str; 16] = [
    "PIC Level",
    "PIE Level",
    "Code Model",
    "Large Data Threshold",
    "uwtable",
    "frame-pointer",
    "Dwarf Version",
    "Debug Info Version",
    "branch-target-enforcement",
    "guarded-control-stack",
    "sign-return-address",
    "sign-return-address-all",
    "sign-return-address-with-bkey",
    "branch-protection-pauth-lr",
    "cf-protection-branch",
    "cf-protection-return",
];

const KNOWN: [Known; 16] = [
    Known::PicLevel,
    Known::PieLevel,
    Known::CodeModel,
    Known::LargeDataThreshold,
    Known::UwTable,
    Known::FramePointer,
    Known::DwarfVersion,
    Known::DebugInfoVersion,
    Known::Bti,
    Known::Gcs,
    Known::PacRet,
    Known::PacRetAll,
    Known::PacBKey,
    Known::PacPauthLr,
    Known::CetIbt,
    Known::CetShstk,
];

/// The names of the ignorable flags (§2.1): `wchar_size` and
/// `min_enum_size` go into ARM EABI build attributes only; `probe-stack`
/// and `stack-probe-size` are defaults for functions LLVM itself
/// synthesizes, and every function a producer delivers has the attribute.
pub const IGNORABLE_MODULE_FLAGS: [&str; 4] =
    ["wchar_size", "min_enum_size", "probe-stack", "stack-probe-size"];

fn known(name: &[u8]) -> Option<Known> {
    KNOWN_MODULE_FLAGS.iter().position(|known| known.as_bytes() == name).map(|at| KNOWN[at])
}

/// The class of the module flag called `name` (§2.1).
pub fn module_flag_class(name: &[u8]) -> FlagClass {
    if known(name).is_some() {
        FlagClass::Known
    } else if IGNORABLE_MODULE_FLAGS.iter().any(|flag| flag.as_bytes() == name) {
        FlagClass::Ignorable
    } else {
        FlagClass::Unknown
    }
}

/// A module flag that does not map onto [`ModuleProps`], as
/// `Module::other_flags` lists it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModuleFlag<'a> {
    /// The flag's name.
    pub name: &'a [u8],
    /// Its behavior.
    pub behavior: FlagBehavior,
    /// Its value.
    pub value: FlagValue<'a>,
    /// Whether hod may ignore it: its name is in
    /// [`IGNORABLE_MODULE_FLAGS`] and it is not a `Require`.
    pub ignorable: bool,
}

#[derive(Clone, Copy, Debug)]
enum StoredValue {
    Int(u64),
    Str(StrId),
    Other,
}

#[derive(Clone, Copy, Debug)]
struct Stored {
    name: StrId,
    behavior: FlagBehavior,
    value: StoredValue,
    ignorable: bool,
}

/// The module flags seen so far.
#[derive(Clone, Debug, Default)]
pub(crate) struct FlagTable {
    /// One bit per [`Known`] flag that has been set.
    seen: u32,
    others: Vec<Stored>,
    strings: StrPool,
}

fn bad<T>(name: &[u8], what: &str) -> IrResult<T> {
    err(
        IrErrorKind::InvalidModuleFlag,
        format!("module flag \"{}\": {what}", String::from_utf8_lossy(name)),
    )
}

impl FlagTable {
    /// Records one module flag; see `Module::set_module_flag`.
    pub(crate) fn set(
        &mut self,
        props: &mut ModuleProps,
        behavior: u32,
        name: &[u8],
        value: FlagValue<'_>,
    ) -> IrResult<()> {
        let Some(behavior) = FlagBehavior::from_llvm(behavior) else {
            return bad(name, &format!("behavior {behavior} is not 1 to 8"));
        };
        // The value shapes LLVM's verifier wants (§2.1): a node (for
        // `require`, a pair) for the behaviors that combine or check
        // values, an integer for two flags it reads.
        let node = matches!(value, FlagValue::Other);
        if matches!(
            behavior,
            FlagBehavior::Require | FlagBehavior::Append | FlagBehavior::AppendUnique
        ) && !node
        {
            return bad(name, "a require, append or append-unique flag needs a metadata node");
        }
        if behavior == FlagBehavior::Require {
            return self.keep(behavior, name, value, false);
        }
        if matches!(name, b"wchar_size" | b"SemanticInterposition")
            && !matches!(value, FlagValue::Int(_))
        {
            return bad(name, "the value is not an integer");
        }
        if matches!(behavior, FlagBehavior::Max | FlagBehavior::Min)
            && !matches!(value, FlagValue::Int(_))
        {
            return bad(name, "a max or min flag needs an integer value");
        }
        let known = known(name);
        let seen = match known {
            Some(flag) => self.seen & (1 << flag as u32) != 0,
            None => self.strings.find(name).is_some_and(|id| {
                self.others.iter().any(|f| f.name == id && f.behavior != FlagBehavior::Require)
            }),
        };
        if seen {
            return bad(name, "set twice");
        }
        let Some(flag) = known else {
            let ignorable = module_flag_class(name) == FlagClass::Ignorable;
            return self.keep(behavior, name, value, ignorable);
        };
        let FlagValue::Int(int) = value else {
            return bad(name, "the value is not an integer");
        };
        apply(props, flag, name, int)?;
        self.seen |= 1 << flag as u32;
        Ok(())
    }

    fn keep(
        &mut self,
        behavior: FlagBehavior,
        name: &[u8],
        value: FlagValue<'_>,
        ignorable: bool,
    ) -> IrResult<()> {
        let value = match value {
            FlagValue::Int(int) => StoredValue::Int(int),
            FlagValue::Str(text) => StoredValue::Str(self.strings.intern(text)?),
            FlagValue::Other => StoredValue::Other,
        };
        let name = self.strings.intern(name)?;
        self.others.push(Stored { name, behavior, value, ignorable });
        Ok(())
    }

    /// The flags that did not map onto the properties, in the order set.
    pub(crate) fn others(&self) -> impl ExactSizeIterator<Item = ModuleFlag<'_>> {
        self.others.iter().map(|flag| ModuleFlag {
            name: self.strings.get(flag.name),
            behavior: flag.behavior,
            value: match flag.value {
                StoredValue::Int(int) => FlagValue::Int(int),
                StoredValue::Str(text) => FlagValue::Str(self.strings.get(text)),
                StoredValue::Other => FlagValue::Other,
            },
            ignorable: flag.ignorable,
        })
    }
}

/// Sets the property that the known flag `flag` with value `int` means.
fn apply(props: &mut ModuleProps, flag: Known, name: &[u8], int: u64) -> IrResult<()> {
    let level = |int| match int {
        0 => Ok(PicLevel::None),
        1 => Ok(PicLevel::Small),
        2 => Ok(PicLevel::Big),
        _ => bad(name, &format!("{int} is not a level (0, 1 or 2)")),
    };
    let protection = |props: &mut ModuleProps, bits| {
        props.branch_protection.set(bits, int != 0);
    };
    match flag {
        Known::PicLevel => props.pic = level(int)?,
        Known::PieLevel => props.pie = level(int)?,
        Known::CodeModel => {
            props.code_model = Some(match int {
                0 => CodeModel::Tiny,
                1 => CodeModel::Small,
                2 => CodeModel::Kernel,
                3 => CodeModel::Medium,
                4 => CodeModel::Large,
                _ => return bad(name, &format!("{int} is not a code model (0 to 4)")),
            });
        }
        Known::LargeDataThreshold => props.large_data_threshold = Some(int),
        Known::UwTable => {
            let Some(kind) = UwTable::from_llvm(int) else {
                return bad(name, &format!("{int} is not an unwind-table kind (0, 1 or 2)"));
            };
            props.uwtable = kind;
        }
        Known::FramePointer => {
            props.frame_pointer = match int {
                0 => FramePointer::None,
                1 => FramePointer::NonLeaf,
                2 => FramePointer::All,
                3 => FramePointer::Reserved,
                4 => FramePointer::NonLeafNoReserve,
                _ => return bad(name, &format!("{int} is not a frame-pointer kind (0 to 4)")),
            };
        }
        Known::DwarfVersion => {
            let Ok(version) = u16::try_from(int) else {
                return bad(name, &format!("{int} is not a DWARF version"));
            };
            props.dwarf_version = Some(version);
        }
        Known::DebugInfoVersion => {
            let Ok(version) = u32::try_from(int) else {
                return bad(name, &format!("{int} is not a debug-info version"));
            };
            props.debug_info_version = Some(version);
        }
        Known::Bti => protection(props, BranchProtection::BTI),
        Known::Gcs => protection(props, BranchProtection::GCS),
        Known::PacRet => protection(props, BranchProtection::PAC_RET),
        Known::PacRetAll => protection(props, BranchProtection::PAC_RET_ALL),
        Known::PacBKey => protection(props, BranchProtection::PAC_B_KEY),
        Known::PacPauthLr => protection(props, BranchProtection::PAC_PAUTH_LR),
        Known::CetIbt => protection(props, BranchProtection::CET_IBT),
        Known::CetShstk => protection(props, BranchProtection::CET_SHSTK),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(
        table: &mut FlagTable,
        props: &mut ModuleProps,
        behavior: u32,
        name: &str,
        value: u64,
    ) -> IrResult<()> {
        table.set(props, behavior, name.as_bytes(), FlagValue::Int(value))
    }

    /// §2.1: each known flag sets its property; these are the flags and
    /// values clang 22 writes for `-fPIE -mbranch-protection=standard`
    /// (AArch64) and `-fcf-protection -mcmodel=medium` (x86-64).
    #[test]
    fn known_flags_map_onto_properties() {
        let mut table = FlagTable::default();
        let mut props = ModuleProps::default();
        assert_eq!(props.pic, PicLevel::None);
        assert_eq!(props.dwarf_version, None);
        assert_eq!(props.branch_protection, BranchProtection::NONE);
        for (behavior, name, value) in [
            (7, "Dwarf Version", 5),
            (2, "Debug Info Version", 3),
            (8, "branch-target-enforcement", 2),
            (8, "guarded-control-stack", 2),
            (8, "sign-return-address", 2),
            (8, "sign-return-address-all", 1),
            (8, "sign-return-address-with-bkey", 0),
            (8, "branch-protection-pauth-lr", 2),
            (8, "cf-protection-return", 1),
            (8, "cf-protection-branch", 1),
            (8, "PIC Level", 2),
            (7, "PIE Level", 1),
            (7, "uwtable", 2),
            (7, "frame-pointer", 4),
            (1, "Code Model", 3),
            (1, "Large Data Threshold", 65536),
        ] {
            set(&mut table, &mut props, behavior, name, value).unwrap();
        }
        let all = BranchProtection::BTI
            | BranchProtection::GCS
            | BranchProtection::PAC_RET
            | BranchProtection::PAC_RET_ALL
            | BranchProtection::PAC_PAUTH_LR
            | BranchProtection::CET_IBT
            | BranchProtection::CET_SHSTK;
        assert_eq!(
            props,
            ModuleProps {
                pic: PicLevel::Big,
                pie: PicLevel::Small,
                code_model: Some(CodeModel::Medium),
                large_data_threshold: Some(65536),
                uwtable: UwTable::Async,
                frame_pointer: FramePointer::NonLeafNoReserve,
                branch_protection: BranchProtection::from_bits(all),
                dwarf_version: Some(5),
                debug_info_version: Some(3),
            }
        );
        assert!(props.branch_protection.has(BranchProtection::BTI | BranchProtection::GCS));
        assert!(!props.branch_protection.has(BranchProtection::PAC_B_KEY));
        assert_eq!(props.branch_protection.bits(), all);
        assert_eq!(table.others().len(), 0);
        assert_eq!(size_of::<ModuleProps>(), 40);
    }

    /// §2.1: every value of the enumerated flags, and the first value
    /// past each.
    #[test]
    fn known_flag_values_are_checked() {
        let fresh = |name: &str, value: u64| {
            let mut props = ModuleProps::default();
            set(&mut FlagTable::default(), &mut props, 1, name, value).map(|()| props)
        };
        let levels = [PicLevel::None, PicLevel::Small, PicLevel::Big];
        for (value, level) in levels.into_iter().enumerate() {
            assert_eq!(fresh("PIC Level", value as u64).unwrap().pic, level);
            assert_eq!(fresh("PIE Level", value as u64).unwrap().pie, level);
        }
        let models = [
            CodeModel::Tiny,
            CodeModel::Small,
            CodeModel::Kernel,
            CodeModel::Medium,
            CodeModel::Large,
        ];
        for (value, model) in models.into_iter().enumerate() {
            assert_eq!(fresh("Code Model", value as u64).unwrap().code_model, Some(model));
        }
        let kinds = [UwTable::None, UwTable::Sync, UwTable::Async];
        for (value, kind) in kinds.into_iter().enumerate() {
            assert_eq!(fresh("uwtable", value as u64).unwrap().uwtable, kind);
        }
        let pointers = [
            FramePointer::None,
            FramePointer::NonLeaf,
            FramePointer::All,
            FramePointer::Reserved,
            FramePointer::NonLeafNoReserve,
        ];
        for (value, pointer) in pointers.into_iter().enumerate() {
            assert_eq!(fresh("frame-pointer", value as u64).unwrap().frame_pointer, pointer);
        }
        assert_eq!(fresh("Dwarf Version", 4).unwrap().dwarf_version, Some(4));
        assert_eq!(
            fresh("Large Data Threshold", u64::MAX).unwrap().large_data_threshold,
            Some(u64::MAX)
        );
        for (name, value) in [
            ("PIC Level", 3),
            ("PIE Level", 3),
            ("Code Model", 5),
            ("uwtable", 3),
            ("frame-pointer", 5),
            ("Dwarf Version", 1 << 16),
            ("Debug Info Version", 1 << 32),
        ] {
            assert_eq!(fresh(name, value).unwrap_err().kind, IrErrorKind::InvalidModuleFlag);
        }
        // A protection flag is on for any value but zero.
        for value in [1, 2, 7] {
            let on = fresh("cf-protection-branch", value).unwrap().branch_protection;
            assert_eq!(on.bits(), BranchProtection::CET_IBT);
        }
        assert_eq!(fresh("cf-protection-branch", 0).unwrap().branch_protection.bits(), 0);
    }

    /// §2.1: a flag hod does not know is kept by name, with its behavior
    /// and value, and marked ignorable only if it is on the list.
    #[test]
    fn other_flags_are_kept_by_name() {
        let mut table = FlagTable::default();
        let mut props = ModuleProps::default();
        set(&mut table, &mut props, 1, "wchar_size", 4).unwrap();
        set(&mut table, &mut props, 7, "RtLibUseGOT", 1).unwrap();
        table.set(&mut props, 4, b"probe-stack", FlagValue::Str(b"inline-asm")).unwrap();
        table.set(&mut props, 5, b"CG Profile", FlagValue::Other).unwrap();
        table.set(&mut props, 3, b"wchar_size", FlagValue::Other).unwrap();
        table.set(&mut props, 3, b"PIC Level", FlagValue::Other).unwrap();
        assert_eq!(props, ModuleProps::default());
        let flags: Vec<ModuleFlag<'_>> = table.others().collect();
        let flag = |name: &'static str, behavior, value, ignorable| ModuleFlag {
            name: name.as_bytes(),
            behavior,
            value,
            ignorable,
        };
        assert_eq!(
            flags,
            [
                flag("wchar_size", FlagBehavior::Error, FlagValue::Int(4), true),
                flag("RtLibUseGOT", FlagBehavior::Max, FlagValue::Int(1), false),
                flag("probe-stack", FlagBehavior::Override, FlagValue::Str(b"inline-asm"), true),
                flag("CG Profile", FlagBehavior::Append, FlagValue::Other, false),
                flag("wchar_size", FlagBehavior::Require, FlagValue::Other, false),
                flag("PIC Level", FlagBehavior::Require, FlagValue::Other, false),
            ]
        );
        assert_eq!(module_flag_class(b"PIC Level"), FlagClass::Known);
        assert_eq!(module_flag_class(b"wchar_size"), FlagClass::Ignorable);
        assert_eq!(module_flag_class(b"RtLibUseGOT"), FlagClass::Unknown);
        assert_eq!(module_flag_class(b"pic level"), FlagClass::Unknown);
        for name in KNOWN_MODULE_FLAGS {
            assert_eq!(module_flag_class(name.as_bytes()), FlagClass::Known);
        }
        for name in IGNORABLE_MODULE_FLAGS {
            assert_eq!(module_flag_class(name.as_bytes()), FlagClass::Ignorable);
        }
    }

    /// §2.1 `InvalidModuleFlag`: what LLVM's verifier refuses in a flag is
    /// an error, and a refused flag changes nothing.
    #[test]
    fn malformed_flags_are_errors() {
        let mut table = FlagTable::default();
        let mut props = ModuleProps::default();
        let kind = |r: IrResult<()>| r.unwrap_err().kind;
        for behavior in [0, 9, u32::MAX] {
            assert_eq!(
                kind(set(&mut table, &mut props, behavior, "x", 1)),
                IrErrorKind::InvalidModuleFlag
            );
        }
        for behavior in [7, 8] {
            for value in [FlagValue::Str(b"big"), FlagValue::Other] {
                assert_eq!(
                    kind(table.set(&mut props, behavior, b"x", value)),
                    IrErrorKind::InvalidModuleFlag
                );
            }
        }
        for value in [FlagValue::Str(b"big"), FlagValue::Other] {
            assert_eq!(
                kind(table.set(&mut props, 1, b"PIC Level", value)),
                IrErrorKind::InvalidModuleFlag
            );
        }
        assert_eq!(table.others().len(), 0);
        assert_eq!(props, ModuleProps::default());

        // A name is set once, unless the other flag is a `Require`.
        set(&mut table, &mut props, 1, "x", 1).unwrap();
        assert_eq!(kind(set(&mut table, &mut props, 1, "x", 1)), IrErrorKind::InvalidModuleFlag);
        table.set(&mut props, 3, b"x", FlagValue::Other).unwrap();
        table.set(&mut props, 3, b"y", FlagValue::Other).unwrap();
        set(&mut table, &mut props, 1, "y", 1).unwrap();
        set(&mut table, &mut props, 8, "PIC Level", 2).unwrap();
        assert_eq!(
            kind(set(&mut table, &mut props, 8, "PIC Level", 1)),
            IrErrorKind::InvalidModuleFlag
        );
        assert_eq!(props.pic, PicLevel::Big);
        assert_eq!(table.others().len(), 4);
        // A value out of range does not count as set.
        assert_eq!(
            kind(set(&mut table, &mut props, 1, "Code Model", 9)),
            IrErrorKind::InvalidModuleFlag
        );
        set(&mut table, &mut props, 1, "Code Model", 1).unwrap();
        assert_eq!(props.code_model, Some(CodeModel::Small));
    }
}
