//! hod's own `.mason.meta` unit (`spec/codegen.md` §9.7.5, WP-332) in
//! the comparisons of hod's objects with `llc`'s: the one difference
//! that rule permits, and the only thing the harnesses leave out for it.

/// `SHT_PROGBITS`.
const SHT_PROGBITS: u32 = 1;
/// `SHF_EXCLUDE`.
const SHF_EXCLUDE: u64 = 0x8000_0000;

/// Where hod's unit starts in a section, or `None` when the section has
/// none: the section must be named `.mason.meta`, be `SHT_PROGBITS` with
/// flags exactly `SHF_EXCLUDE`, and end with a unit (`MASN`, mortar's
/// `spec/meta.md` MT-10) at an 8-aligned offset whose producer is 2
/// (hod). A comparison keeps `data[..start]` (kiln's units, compared as
/// any bytes) and leaves out the section whole when `start` is 0.
pub fn hods_unit_start(name: &[u8], sh_type: u32, flags: u64, data: &[u8]) -> Option<usize> {
    if name != b".mason.meta" || sh_type != SHT_PROGBITS || flags != SHF_EXCLUDE {
        return None;
    }
    let u16_at = |at: usize| u16::from_le_bytes([data[at], data[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    (0..data.len()).step_by(8).find(|&at| {
        data.len() >= at + 32
            && &data[at..at + 4] == b"MASN"
            && u16_at(at + 28) == 2
            && at + u32_at(at + 8) as usize == data.len()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(producer: u8, size: u8) -> Vec<u8> {
        let mut unit = vec![0u8; usize::from(size)];
        unit[..4].copy_from_slice(b"MASN");
        unit[8] = size;
        unit[28] = producer;
        unit
    }

    #[test]
    fn finds_only_hods_unit_at_the_end_of_the_container() {
        let hod = unit(2, 88);
        assert_eq!(hods_unit_start(b".mason.meta", 1, SHF_EXCLUDE, &hod), Some(0));
        let mut both = unit(1, 40);
        both.extend_from_slice(&hod);
        assert_eq!(hods_unit_start(b".mason.meta", 1, SHF_EXCLUDE, &both), Some(40));
        let kiln = unit(1, 40);
        assert_eq!(hods_unit_start(b".mason.meta", 1, SHF_EXCLUDE, &kiln), None);
        // Not the container: another name, type or flags.
        assert_eq!(hods_unit_start(b".mason.metb", 1, SHF_EXCLUDE, &hod), None);
        assert_eq!(hods_unit_start(b".mason.meta", 7, SHF_EXCLUDE, &hod), None);
        assert_eq!(hods_unit_start(b".mason.meta", 1, SHF_EXCLUDE | 0x20_0000, &hod), None);
        assert_eq!(hods_unit_start(b".mason.meta", 1, SHF_EXCLUDE, &hod[..80]), None);
    }
}
