//! What differs between two ELF objects, by section (`spec/harness.md`
//! "compare-binaries"). A reader of ELF64 little-endian section headers
//! only: names, types and contents, nothing interpreted.

use std::collections::BTreeMap;

/// `SHT_NOBITS`: a section without bytes in the file (its size is compared).
const SHT_NOBITS: u32 = 8;

/// One section: its contents, or for `SHT_NOBITS` its size as text.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Contents<'a> {
    Bytes(&'a [u8]),
    Size(u64),
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

fn slice(data: &[u8], offset: u64, size: u64) -> Option<&[u8]> {
    let start = usize::try_from(offset).ok()?;
    let end = start.checked_add(usize::try_from(size).ok()?)?;
    data.get(start..end)
}

/// The sections of an ELF64 little-endian object, keyed by name; the
/// second and later sections of one name get `#<n>` (`.text#2`). `None`
/// when the data is not such an object.
fn sections(data: &[u8]) -> Option<BTreeMap<String, Contents<'_>>> {
    if data.get(..4)? != b"\x7fELF" || *data.get(4)? != 2 || *data.get(5)? != 1 {
        return None;
    }
    let shoff = usize::try_from(u64_at(data, 0x28)?).ok()?;
    let entsize = usize::from(u16_at(data, 0x3a)?);
    let count = usize::from(u16_at(data, 0x3c)?);
    let strndx = usize::from(u16_at(data, 0x3e)?);
    if entsize < 0x40 {
        return None;
    }
    let header = |i: usize| -> Option<(u32, u32, u64, u64)> {
        let at = shoff.checked_add(i.checked_mul(entsize)?)?;
        Some((
            u32_at(data, at)?,
            u32_at(data, at + 4)?,
            u64_at(data, at + 0x18)?,
            u64_at(data, at + 0x20)?,
        ))
    };
    let (_, _, str_offset, str_size) = header(strndx)?;
    let names = slice(data, str_offset, str_size)?;
    let mut found = BTreeMap::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for i in 1..count {
        let (name, kind, offset, size) = header(i)?;
        let name = names.get(usize::try_from(name).ok()?..)?;
        let name = String::from_utf8_lossy(&name[..name.iter().position(|b| *b == 0)?]);
        let n = seen.entry(name.to_string()).or_insert(0);
        *n += 1;
        let key = if *n == 1 { name.to_string() } else { format!("{name}#{n}") };
        let contents = if kind == SHT_NOBITS {
            Contents::Size(size)
        } else {
            Contents::Bytes(slice(data, offset, size)?)
        };
        found.insert(key, contents);
    }
    Some(found)
}

/// The section names of `x` that `y` does not have.
fn only<'a>(
    x: &'a BTreeMap<String, Contents<'_>>,
    y: &BTreeMap<String, Contents<'_>>,
) -> Vec<&'a str> {
    x.keys().filter(|k| !y.contains_key(*k)).map(String::as_str).collect()
}

/// What differs between two objects that are not byte-equal: the sections
/// whose contents differ and those only one has, by name, or the headers
/// when every section is equal.
pub fn difference(base: &[u8], head: &[u8]) -> String {
    let (Some(a), Some(b)) = (sections(base), sections(head)) else {
        let at =
            base.iter().zip(head).position(|(x, y)| x != y).unwrap_or(base.len().min(head.len()));
        return format!(
            "not ELF64 objects: {} and {} bytes, first difference at byte {at}",
            base.len(),
            head.len()
        );
    };
    let differ: Vec<&str> = a
        .iter()
        .filter(|(k, v)| b.get(*k).is_some_and(|w| w != *v))
        .map(|(k, _)| k.as_str())
        .collect();
    let (only_base, only_head) = (only(&a, &b), only(&b, &a));
    let mut parts = Vec::new();
    if !differ.is_empty() {
        parts.push(format!("sections differ: {}", differ.join(" ")));
    }
    if !only_base.is_empty() {
        parts.push(format!("only in base: {}", only_base.join(" ")));
    }
    if !only_head.is_empty() {
        parts.push(format!("only in head: {}", only_head.join(" ")));
    }
    if parts.is_empty() {
        parts.push("every section is equal: the ELF header or section table differs".to_owned());
    }
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal ELF64 object with sections `names`/`contents`.
    pub(crate) fn object(sections: &[(&str, &[u8])]) -> Vec<u8> {
        let mut strtab = vec![0u8];
        let mut offsets = Vec::new();
        for (name, _) in sections.iter().copied().chain([(".shstrtab", &b""[..])]) {
            offsets.push(u32::try_from(strtab.len()).unwrap());
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
        }
        let mut data = vec![0u8; 0x40];
        data[..6].copy_from_slice(b"\x7fELF\x02\x01");
        let mut placed = Vec::new();
        for (_, bytes) in sections {
            placed.push((data.len() as u64, bytes.len() as u64));
            data.extend_from_slice(bytes);
        }
        placed.push((data.len() as u64, strtab.len() as u64));
        data.extend_from_slice(&strtab);
        let shoff = data.len() as u64;
        data.extend_from_slice(&[0u8; 0x40]);
        for (i, (offset, size)) in placed.iter().enumerate() {
            let mut h = [0u8; 0x40];
            h[..4].copy_from_slice(&offsets[i].to_le_bytes());
            h[4..8].copy_from_slice(&1u32.to_le_bytes());
            h[0x18..0x20].copy_from_slice(&offset.to_le_bytes());
            h[0x20..0x28].copy_from_slice(&size.to_le_bytes());
            data.extend_from_slice(&h);
        }
        data[0x28..0x30].copy_from_slice(&shoff.to_le_bytes());
        data[0x3a..0x3c].copy_from_slice(&0x40u16.to_le_bytes());
        let count = u16::try_from(placed.len() + 1).unwrap();
        data[0x3c..0x3e].copy_from_slice(&count.to_le_bytes());
        data[0x3e..0x40].copy_from_slice(&(count - 1).to_le_bytes());
        data
    }

    #[test]
    fn names_the_sections_that_differ() {
        let a = object(&[(".text", b"\x90\x90"), (".data", b"ab"), (".text", b"x")]);
        let b = object(&[(".text", b"\x90\xc3"), (".data", b"ab"), (".text", b"y"), (".bss", b"")]);
        // The names differ too: the section names' string table.
        assert_eq!(
            difference(&a, &b),
            "sections differ: .shstrtab .text .text#2; only in head: .bss"
        );
        let d = object(&[(".text", b"\x90\xc3"), (".data", b"ab"), (".text", b"x")]);
        assert_eq!(difference(&a, &d), "sections differ: .text");
        assert_eq!(
            difference(b"abc", b"abd"),
            "not ELF64 objects: 3 and 3 bytes, first difference at byte 2"
        );
        let mut c = a.clone();
        c[0x10] = 1;
        assert!(difference(&a, &c).contains("ELF header"));
    }
}
