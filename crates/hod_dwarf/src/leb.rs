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
        if (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0) {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dwarf_examples() {
        // DWARF 5 §7.6, figures 22 and 23.
        let u = |v| {
            let mut o = Vec::new();
            write_uleb(&mut o, v);
            o
        };
        let s = |v| {
            let mut o = Vec::new();
            write_sleb(&mut o, v);
            o
        };
        assert_eq!(u(2), [2]);
        assert_eq!(u(127), [127]);
        assert_eq!(u(128), [0x80, 1]);
        assert_eq!(u(129), [0x81, 1]);
        assert_eq!(u(12857), [0xb9, 0x64]);
        assert_eq!(s(2), [2]);
        assert_eq!(s(-2), [0x7e]);
        assert_eq!(s(127), [0xff, 0]);
        assert_eq!(s(-127), [0x81, 0x7f]);
        assert_eq!(s(128), [0x80, 1]);
        assert_eq!(s(-128), [0x80, 0x7f]);
    }
}
