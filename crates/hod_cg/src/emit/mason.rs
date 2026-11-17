//! hod's own `.mason.meta` unit (`spec/codegen.md` §9.7.5, WP-332): one
//! kind-1 block with the row `reloc-model`, laid out as the mortar
//! linker's `spec/meta.md` MT-10, MT-11, MT-12 and MT-26 say. The quotes
//! next to the fields are from that specification.

use crate::access::ModelUsed;

/// The section's name (MT-1).
pub(super) const SECTION: &[u8] = b".mason.meta";

/// MT-10: "Every structure below is 8-aligned and a multiple of 8 bytes".
pub(super) const ALIGN: u64 = 8;

/// MT-10 header size: "`u32 header_size` | 32 in 1.0".
const HEADER_SIZE: u32 = 32;
/// MT-11: the block header is 16 bytes ("16 | rows").
const BLOCK_HEADER_SIZE: u32 = 16;
/// MT-26: "Rows `{u32 name; u32 value; u32 flags; u32 reserved}`".
const ROW_SIZE: u32 = 16;
/// The row's name (MT-26 table, `reloc-model` [hod]).
const NAME: &[u8] = b"reloc-model";

/// The bytes of hod's unit for `model`. Deterministic: no time, path or
/// host enters it (MT-38).
pub(super) fn unit(model: ModelUsed) -> Vec<u8> {
    let value = model.name();
    // MT-12: "offset 0 is the empty string"; strings are laid out "in the
    // order the sorted rows first reference them" (MT-38): name, value.
    let mut strtab = vec![0u8];
    let name_at = strtab.len() as u32;
    strtab.extend_from_slice(NAME);
    strtab.push(0);
    let value_at = strtab.len() as u32;
    strtab.extend_from_slice(value);
    strtab.push(0);
    let block_size = BLOCK_HEADER_SIZE + ROW_SIZE;
    let strtab_offset = HEADER_SIZE + block_size;
    let strtab_size = strtab.len() as u32;
    // MT-10: "`u32 unit_size` | bytes of the unit including this header;
    // a multiple of 8"; the bytes past the string table are zero.
    let unit_size = (strtab_offset + strtab_size).next_multiple_of(ALIGN as u32);

    let mut out = Vec::with_capacity(unit_size as usize);
    let u16 = |out: &mut Vec<u8>, v: u16| out.extend_from_slice(&v.to_le_bytes());
    let u32 = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
    // MT-10 header. "All integers are little-endian, in every ELF class".
    out.extend_from_slice(b"MASN"); // 0: `u8[4] magic` | `MASN`
    u16(&mut out, 1); // 4: `u16 major` | 1
    u16(&mut out, 0); // 6: `u16 minor` | 0
    u32(&mut out, unit_size); // 8: `u32 unit_size`
    u32(&mut out, HEADER_SIZE); // 12: `u32 header_size` | 32 in 1.0
    u32(&mut out, 1); // 16: `u32 block_count`
    u32(&mut out, strtab_offset); // 20: `u32 strtab_offset`
    u32(&mut out, strtab_size); // 24: `u32 strtab_size`
    u16(&mut out, 2); // 28: `u16 producer` | 1 kiln, 2 hod, ...
    u16(&mut out, 0); // 30: `u16 flags` | 0 in 1.0
    // MT-11 block header, at `header_size`.
    u16(&mut out, 1); // 0: `u16 kind` (MT-26: kind 1, TU summary)
    u16(&mut out, 1); // 2: `u16 version` | of the kind's row layout, from 1
    u32(&mut out, block_size); // 4: `u32 block_size` | bytes including this header
    u32(&mut out, ROW_SIZE); // 8: `u32 row_size` | a multiple of 8
    u32(&mut out, 1); // 12: `u32 row_count`
    // MT-26 row `{u32 name; u32 value; u32 flags; u32 reserved}`.
    u32(&mut out, name_at); // name: a string (MT-12 offset)
    u32(&mut out, value_at); // value: a string (MT-12 offset)
    u32(&mut out, 0); // flags: bit 0 `ENFORCE`; `reloc-model` is "enforce no"
    u32(&mut out, 0); // reserved
    out.extend_from_slice(&strtab);
    out.resize(unit_size as usize, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: [u8; 32] = [
        b'M', b'A', b'S', b'N', 1, 0, 0, 0, // magic, major 1, minor 0
        88, 0, 0, 0, 32, 0, 0, 0, // unit_size, header_size
        1, 0, 0, 0, 64, 0, 0, 0, // block_count, strtab_offset
        0, 0, 0, 0, 2, 0, 0, 0, // strtab_size (patched), producer 2, flags 0
    ];
    const BLOCK: [u8; 32] = [
        1, 0, 1, 0, 32, 0, 0, 0, // kind 1, version 1, block_size 32
        16, 0, 0, 0, 1, 0, 0, 0, // row_size 16, row_count 1
        1, 0, 0, 0, 13, 0, 0, 0, // name 1, value 13
        0, 0, 0, 0, 0, 0, 0, 0, // flags 0, reserved 0
    ];

    fn expected(value: &[u8]) -> Vec<u8> {
        let mut head = HEAD;
        head[24] = (1 + NAME.len() + 1 + value.len() + 1) as u8;
        let mut out = head.to_vec();
        out.extend_from_slice(&BLOCK);
        out.extend_from_slice(b"\0reloc-model\0");
        out.extend_from_slice(value);
        out.push(0);
        out.resize(88, 0);
        out
    }

    #[test]
    fn static_unit() {
        let bytes = unit(ModelUsed::Static);
        assert_eq!(bytes, expected(b"static"));
        assert_eq!(bytes[24], 20);
        assert_eq!(&bytes[64..84], b"\0reloc-model\0static\0");
        assert_eq!(bytes[84..], [0; 4]);
    }

    #[test]
    fn pie_unit() {
        let bytes = unit(ModelUsed::Pie);
        assert_eq!(bytes, expected(b"pie"));
        assert_eq!(bytes[24], 17);
        assert_eq!(bytes[81..], [0; 7]);
    }

    #[test]
    fn pic_unit() {
        let bytes = unit(ModelUsed::Pic);
        assert_eq!(bytes, expected(b"pic"));
        assert_eq!(&bytes[64..81], b"\0reloc-model\0pic\0");
    }

    #[test]
    fn every_unit_is_a_multiple_of_8() {
        for model in [ModelUsed::Static, ModelUsed::Pie, ModelUsed::Pic] {
            assert_eq!(unit(model).len() % 8, 0);
        }
    }
}
