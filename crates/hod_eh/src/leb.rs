//! LEB128 (DWARF 5 §7.6), appended to a byte buffer.

/// Where bytes are encoded: the section's buffer, or the FDE staging buffer on the hot
/// path, which keeps a function's bytes on the stack and appends them to
/// the section in one copy.
pub(crate) trait Sink {
    fn byte(&mut self, b: u8);
    fn bytes(&mut self, b: &[u8]);
}

impl Sink for Vec<u8> {
    #[inline]
    fn byte(&mut self, b: u8) {
        self.push(b);
    }
    #[inline]
    fn bytes(&mut self, b: &[u8]) {
        self.extend_from_slice(b);
    }
}

#[inline]
pub(crate) fn uleb<S: Sink>(out: &mut S, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.byte(byte);
            return;
        }
        out.byte(byte | 0x80);
    }
}

#[inline]
pub(crate) fn sleb<S: Sink>(out: &mut S, mut value: i64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0) {
            out.byte(byte);
            return;
        }
        out.byte(byte | 0x80);
    }
}

/// Appends `value` as unsigned LEB128.
pub fn write_uleb(out: &mut Vec<u8>, value: u64) {
    uleb(out, value);
}

/// Appends `value` as signed LEB128.
pub fn write_sleb(out: &mut Vec<u8>, value: i64) {
    sleb(out, value);
}

/// The number of bytes [`write_uleb`] appends for `value`.
pub const fn uleb_len(value: u64) -> usize {
    let bits = 64 - (value | 1).leading_zeros() as usize;
    bits.div_ceil(7)
}
