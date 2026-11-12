//! LEB128 (DWARF 5 §7.6), appended to a byte buffer.

/// Appends `value` as unsigned LEB128.
#[inline]
pub fn write_uleb(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Appends `value` as signed LEB128.
#[inline]
pub fn write_sleb(out: &mut Vec<u8>, mut value: i64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
        if done {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// The number of bytes [`write_uleb`] appends for `value`.
pub const fn uleb_len(value: u64) -> usize {
    let bits = 64 - (value | 1).leading_zeros() as usize;
    bits.div_ceil(7)
}
