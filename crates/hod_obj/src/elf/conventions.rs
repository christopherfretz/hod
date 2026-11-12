//! Sections whose names and contents are fixed by convention. Contract:
//! `spec/object.md` "ELF conventions".

use crate::error::Error;
use crate::model::{Arch, Object, SectionId, SectionKind};

/// The priority of an initializer or finalizer that was given none. Its
/// section has no priority suffix.
pub const DEFAULT_INIT_PRIORITY: u16 = 65535;

/// `GNU_PROPERTY_AARCH64_FEATURE_1_AND` bit: every function entry and
/// indirect branch target is a BTI landing pad.
pub const GNU_PROPERTY_AARCH64_FEATURE_1_BTI: u32 = 1 << 0;
/// `GNU_PROPERTY_AARCH64_FEATURE_1_AND` bit: return addresses are signed.
pub const GNU_PROPERTY_AARCH64_FEATURE_1_PAC: u32 = 1 << 1;
/// `GNU_PROPERTY_AARCH64_FEATURE_1_AND` bit: compatible with a guarded
/// control stack.
pub const GNU_PROPERTY_AARCH64_FEATURE_1_GCS: u32 = 1 << 2;
/// `GNU_PROPERTY_X86_FEATURE_1_AND` bit: every indirect branch target
/// starts with `ENDBR64`.
pub const GNU_PROPERTY_X86_FEATURE_1_IBT: u32 = 1 << 0;
/// `GNU_PROPERTY_X86_FEATURE_1_AND` bit: compatible with a shadow stack.
pub const GNU_PROPERTY_X86_FEATURE_1_SHSTK: u32 = 1 << 1;

const NT_GNU_PROPERTY_TYPE_0: u32 = 5;
const GNU_PROPERTY_AARCH64_FEATURE_1_AND: u32 = 0xc000_0000;
const GNU_PROPERTY_X86_FEATURE_1_AND: u32 = 0xc000_0002;

/// `<base>` for the default priority, else `<base>.<priority>` with the
/// priority in decimal, as LLVM names these sections. Returns the buffer
/// and the name's length in it.
fn priority_name(base: &[u8; 11], priority: u16) -> ([u8; 17], usize) {
    let mut name = [0; 17];
    name[..11].copy_from_slice(base);
    let mut len = 11;
    if priority != DEFAULT_INIT_PRIORITY {
        name[len] = b'.';
        len += 1;
        let mut divisor = 10_000;
        while divisor > 1 && priority / divisor == 0 {
            divisor /= 10;
        }
        while divisor != 0 {
            name[len] = b'0' + (priority / divisor % 10) as u8;
            len += 1;
            divisor /= 10;
        }
    }
    (name, len)
}

/// Adds a section of pointers to functions that run before `main`, in
/// increasing `priority` order: `.init_array.<priority>`, or `.init_array`
/// for [`DEFAULT_INIT_PRIORITY`]. Each call adds a section.
///
/// # Errors
/// As [`Object::add_section`].
pub fn add_init_array(obj: &mut Object, priority: u16) -> Result<SectionId, Error> {
    let (name, len) = priority_name(b".init_array", priority);
    obj.add_section(&name[..len], SectionKind::InitArray, 8)
}

/// Adds a section of pointers to functions that run at exit, in decreasing
/// `priority` order: `.fini_array.<priority>`, or `.fini_array` for
/// [`DEFAULT_INIT_PRIORITY`]. Each call adds a section.
///
/// # Errors
/// As [`Object::add_section`].
pub fn add_fini_array(obj: &mut Object, priority: u16) -> Result<SectionId, Error> {
    let (name, len) = priority_name(b".fini_array", priority);
    obj.add_section(&name[..len], SectionKind::FiniArray, 8)
}

/// The contents of a `.note.gnu.property` section that declares `features`
/// for every function of an object: the `GNU_PROPERTY_AARCH64_FEATURE_1_*`
/// bits for AArch64, the `GNU_PROPERTY_X86_FEATURE_1_*` bits for x86-64.
pub const fn gnu_property_note(arch: Arch, features: u32) -> [u8; 32] {
    let property = match arch {
        Arch::AArch64 => GNU_PROPERTY_AARCH64_FEATURE_1_AND,
        Arch::X86_64 => GNU_PROPERTY_X86_FEATURE_1_AND,
    };
    // An `Elf64_Nhdr`, the owner, then one property: type, data size, the
    // data, and padding to eight bytes.
    let words =
        [4, 16, NT_GNU_PROPERTY_TYPE_0, u32::from_le_bytes(*b"GNU\0"), property, 4, features, 0];
    let mut note = [0; 32];
    let mut at = 0;
    while at < 32 {
        note[at] = words[at / 4].to_le_bytes()[at % 4];
        at += 1;
    }
    note
}

/// Adds the `.note.gnu.property` section that declares `features` (see
/// [`gnu_property_note`]) for the object's architecture. An object has at
/// most one.
///
/// # Errors
/// As [`Object::add_section`].
pub fn add_gnu_property_note(obj: &mut Object, features: u32) -> Result<SectionId, Error> {
    let section = obj.add_section(b".note.gnu.property", SectionKind::Note, 8)?;
    obj.append(section, &gnu_property_note(obj.arch(), features));
    Ok(section)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `spec/object.md` "ELF conventions": the priority is in decimal
    /// without padding; the default priority has no suffix.
    #[test]
    fn priority_names() {
        let name = |priority| {
            let (name, len) = priority_name(b".init_array", priority);
            String::from_utf8(name[..len].to_vec()).unwrap()
        };
        assert_eq!(name(DEFAULT_INIT_PRIORITY), ".init_array");
        for priority in [0, 9, 10, 101, 1000, 9999, 10_000, 65534] {
            assert_eq!(name(priority), format!(".init_array.{priority}"));
        }
        let (name, len) = priority_name(b".fini_array", 65534);
        assert_eq!(&name[..len], b".fini_array.65534");
    }

    /// "ELF conventions": the property note, field by field.
    #[test]
    fn property_note_layout() {
        let note = gnu_property_note(Arch::X86_64, 3);
        let word = |at: usize| u32::from_le_bytes(note[at..at + 4].try_into().unwrap());
        assert_eq!((word(0), word(4), word(8)), (4, 16, 5));
        assert_eq!(&note[12..16], b"GNU\0");
        assert_eq!((word(16), word(20), word(24), word(28)), (0xc000_0002, 4, 3, 0));
        let note = gnu_property_note(Arch::AArch64, GNU_PROPERTY_AARCH64_FEATURE_1_BTI);
        assert_eq!(note[16..28], [0, 0, 0, 0xc0, 4, 0, 0, 0, 1, 0, 0, 0]);
    }
}
