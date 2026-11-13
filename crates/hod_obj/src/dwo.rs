//! The split-DWARF companion of an object (`spec/object.md` "`Object`",
//! WP-062).
//!
//! `-gsplit-dwarf` makes two files from one compilation: the object, with
//! a skeleton unit, and a `.dwo` file with the unit's DIEs. The DWARF
//! writer builds the `.dwo` as an [`Object`] of its own (sections only:
//! nothing in it is relocated, and it has no symbols) and attaches it to
//! the object it describes; whoever writes the object takes it off and
//! writes it to the `.dwo` path. The ELF writer never looks at it: an
//! object with a companion is written exactly as one without.

use crate::model::Object;

impl Object {
    /// Attaches `dwo` as this object's split-DWARF companion, replacing
    /// any earlier one.
    pub fn set_dwo(&mut self, dwo: Object) {
        self.dwo = Some(Box::new(dwo));
    }

    /// The split-DWARF companion, if one is attached.
    pub fn dwo(&self) -> Option<&Object> {
        self.dwo.as_deref()
    }

    /// Takes the split-DWARF companion off, if one is attached.
    pub fn take_dwo(&mut self) -> Option<Object> {
        self.dwo.take().map(|dwo| *dwo)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Arch, Object, SectionKind, elf};

    #[test]
    fn a_companion_does_not_change_the_object() {
        let mut obj = Object::new(Arch::AArch64);
        let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
        obj.append(text, &[0xc0, 0x03, 0x5f, 0xd6]);
        let alone = elf::to_vec(&obj).unwrap();
        let mut dwo = Object::new(Arch::AArch64);
        let info = dwo.add_section(b".debug_info.dwo", SectionKind::Debug, 1).unwrap();
        dwo.append(info, &[1, 2, 3]);
        obj.set_dwo(dwo);
        assert_eq!(elf::to_vec(&obj).unwrap(), alone);
        assert_eq!(obj.dwo().map(Object::section_count), Some(1));
        let dwo = obj.take_dwo().unwrap();
        assert_eq!(dwo.data(info), [1, 2, 3]);
        assert!(obj.take_dwo().is_none());
    }
}
