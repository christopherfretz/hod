//! Skipping text without lexing it (`spec/ll.md` §4.2).
//!
//! A function body, an initializer in brackets, a metadata node: the
//! index scan only needs to know where each ends. Between an opening
//! bracket and its partner the only bytes that matter are the bracket
//! pair itself and the three that start a region in which brackets do not
//! count: `"` (a string or quoted name, to the next `"`), `;` (a comment,
//! to the end of the line) and `/` (of `/*`, to `*/`). Everything else is
//! stepped over eight bytes at a time: a word of the source is compared
//! with all five bytes at once by arithmetic on the whole word.

const ONES: u64 = 0x0101_0101_0101_0101;
const HIGHS: u64 = 0x8080_8080_8080_8080;

/// A word with the high bit set in every byte of `word` that equals
/// `byte`, and possibly in bytes above such a one: the lowest set bit is
/// exact, which is all a search for the first match needs.
#[inline]
fn matches(word: u64, byte: u8) -> u64 {
    let diff = word ^ (ONES * u64::from(byte));
    diff.wrapping_sub(ONES) & !diff & HIGHS
}

/// The eight bytes of `src` at `at` as a word whose lowest byte is the
/// first.
#[inline]
fn word_at(src: &[u8], at: usize) -> Option<u64> {
    src.get(at..)?.first_chunk::<8>().map(|bytes| u64::from_le_bytes(*bytes))
}

/// The offset of the first `byte` at or after `from`.
#[inline]
pub(crate) fn find_byte(src: &[u8], mut from: usize, byte: u8) -> Option<usize> {
    while let Some(word) = word_at(src, from) {
        let found = matches(word, byte);
        if found != 0 {
            return Some(from + (found.trailing_zeros() / 8) as usize);
        }
        from += 8;
    }
    src.get(from..)?.iter().position(|&b| b == byte).map(|n| from + n)
}

/// The offset just past the `CLOSE` that matches an `OPEN` already
/// consumed: `from` is the offset after that `OPEN`. Only this one pair
/// is counted; in well-formed text every other pair is balanced inside
/// it. `Err` is the offset of the string or comment that does not end,
/// or the length of the source when the group does not.
pub(crate) fn skip_group<const OPEN: u8, const CLOSE: u8>(
    src: &[u8],
    from: usize,
) -> Result<usize, usize> {
    let special = |b: u8| matches!(b, b'"' | b';' | b'/') || b == OPEN || b == CLOSE;
    let specials = |word: u64| {
        matches(word, b'"')
            | matches(word, b';')
            | matches(word, b'/')
            | matches(word, OPEN)
            | matches(word, CLOSE)
    };
    let mut pos = from.min(src.len());
    let mut depth = 1usize;
    loop {
        // The first byte of interest: a word at a time, then the tail.
        loop {
            let Some(word) = word_at(src, pos) else {
                let Some(n) = src[pos..].iter().position(|&b| special(b)) else {
                    return Err(src.len());
                };
                pos += n;
                break;
            };
            let found = specials(word);
            if found != 0 {
                pos += (found.trailing_zeros() / 8) as usize;
                break;
            }
            pos += 8;
        }
        let b = src[pos];
        pos += 1;
        if b == b'"' {
            pos = find_byte(src, pos, b'"').ok_or(pos - 1)? + 1;
        } else if b == b';' {
            pos = find_byte(src, pos, b'\n').ok_or(src.len())?;
        } else if b == b'/' {
            if src.get(pos) == Some(&b'*') {
                pos = comment_end(src, pos + 1).ok_or(pos - 1)?;
            }
        } else if b == OPEN {
            depth += 1;
        } else {
            depth -= 1;
            if depth == 0 {
                return Ok(pos);
            }
        }
    }
}

/// The offset just past the `*/` that ends a comment whose text starts at
/// `from`.
pub(crate) fn comment_end(src: &[u8], mut from: usize) -> Option<usize> {
    loop {
        let star = find_byte(src, from, b'*')?;
        if src.get(star + 1) == Some(&b'/') {
            return Some(star + 2);
        }
        from = star + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn braces(text: &str) -> Result<usize, usize> {
        skip_group::<b'{', b'}'>(text.as_bytes(), 1)
    }

    /// §4.2: nesting, strings, quoted names and both comment forms.
    #[test]
    fn groups() {
        assert_eq!(braces("{}"), Ok(2));
        assert_eq!(braces("{ { } { { } } } x"), Ok(15));
        assert_eq!(braces("{ \"}\" } x"), Ok(7));
        assert_eq!(braces("{ %\"a}b\" = 1 ; }}}\n } x"), Ok(21));
        assert_eq!(braces("{ /* } \" ; */ } x"), Ok(15));
        assert_eq!(braces("{ / } x"), Ok(5));
        assert_eq!(braces("{ ( [ } x"), Ok(7));
        // A quote in a comment and a semicolon in a string do not start
        // anything.
        assert_eq!(braces("{ ; \"\n } x"), Ok(8));
        assert_eq!(braces("{ \";\" } x"), Ok(7));
    }

    /// §4.2: what does not end.
    #[test]
    fn unterminated() {
        assert_eq!(braces("{ { }"), Err(5));
        assert_eq!(braces("{ \"abc }"), Err(2));
        assert_eq!(braces("{ /* abc }"), Err(2));
        assert_eq!(braces("{ ; abc }"), Err(9));
    }

    /// §4.2: the block loop and the byte loop agree wherever the bytes of
    /// interest fall relative to a block boundary.
    #[test]
    fn every_alignment() {
        for pad in 0..40 {
            for tail in [0, 1, 7, 8, 9, 19] {
                let text = format!(
                    "{{{}\"}}{}\"{{{};}}\n{}}}{}}}rest",
                    " ".repeat(pad),
                    "a".repeat(tail),
                    "b".repeat(pad),
                    "c".repeat(tail),
                    " ".repeat(pad)
                );
                let end = braces(&text).unwrap();
                assert_eq!(&text[end..], "rest", "pad {pad} tail {tail}");
            }
        }
        let text = "a".repeat(100) + "\"";
        for from in 0..100 {
            assert_eq!(find_byte(text.as_bytes(), from, b'"'), Some(100));
        }
        assert_eq!(find_byte(text.as_bytes(), 101, b'"'), None);
        assert_eq!(find_byte(text.as_bytes(), 500, b'"'), None);
    }
}
