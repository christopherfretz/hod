//! The modules a lane kept (`HOD_OSS_KEEP_MODULES`, `scripts/oss/hod-shim`;
//! `spec/harness.md` "compare-binaries").
//!
//! A directory of kept modules holds `<hash>.ll`, the module exactly as
//! hod received it on standard input, and `MODULES.tsv`: a header line,
//! then one row per module, `hash`, `bytes`, `output` (the object path of
//! the first compile that kept it), then hod's arguments one per field.
//! A backslash, tab, newline and carriage return in a field are written
//! `\\`, `\t`, `\n`, `\r`.

use std::fs;
use std::path::{Path, PathBuf};

/// The manifest's name.
pub const MANIFEST: &str = "MODULES.tsv";

/// One kept module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub hash: String,
    pub bytes: u64,
    pub output: String,
    pub args: Vec<String>,
}

/// Every `MODULES.tsv` under `dir`, sorted.
pub fn manifests(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let read = fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {e}", dir.display()))?;
    let mut entries: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            manifests(&path, found)?;
        } else if path.file_name().is_some_and(|n| n == MANIFEST) {
            found.push(path);
        }
    }
    Ok(())
}

fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Parses a manifest's text.
pub fn parse_manifest(text: &str) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if number == 0 && line.starts_with("hash\t") || line.is_empty() {
            continue;
        }
        let fields: Vec<String> = line.split('\t').map(unescape).collect();
        let [hash, bytes, output, args @ ..] = fields.as_slice() else {
            return Err(format!("line {}: fewer than three fields", number + 1));
        };
        if hash.is_empty() || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("line {}: '{hash}' is not a hash", number + 1));
        }
        let bytes =
            bytes.parse().map_err(|_| format!("line {}: invalid size '{bytes}'", number + 1))?;
        rows.push(Row { hash: hash.clone(), bytes, output: output.clone(), args: args.to_vec() });
    }
    Ok(rows)
}

/// Reads a manifest.
pub fn read_manifest(path: &Path) -> Result<Vec<Row>, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
    parse_manifest(&text).map_err(|e| format!("'{}': {e}", path.display()))
}

/// The arguments to replay a recorded compile with, and the index of the
/// output path in them (`-o <x>`; a joined `-o<x>` is split). The module
/// must have been read from standard input (`-`).
pub fn replay_args(recorded: &[String]) -> Result<(Vec<String>, usize), String> {
    let mut args = Vec::with_capacity(recorded.len() + 1);
    let mut output = None;
    let mut rest = recorded.iter();
    while let Some(arg) = rest.next() {
        if arg == "-o" {
            let value = rest.next().ok_or("'-o' without a value")?;
            args.push("-o".to_owned());
            output = Some(args.len());
            args.push(value.clone());
        } else if let Some(value) = arg.strip_prefix("-o").filter(|v| !v.is_empty()) {
            args.push("-o".to_owned());
            output = Some(args.len());
            args.push(value.to_owned());
        } else {
            args.push(arg.clone());
        }
    }
    if !args.iter().any(|a| a == "-") {
        return Err("the module was not read from standard input ('-')".to_owned());
    }
    let output = output.ok_or("no '-o <object>' in the arguments")?;
    Ok((args, output))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn manifest_rows() {
        let text = "hash\tbytes\toutput\targs\nab12\t10\t/t/a.o\t--strict\t-o\t/t/a.o\t-\n\
                    cd34\t5\tx\\ty.o\t-oz\\\\w\t-\n";
        let rows = parse_manifest(text).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].args, strings(&["--strict", "-o", "/t/a.o", "-"]));
        assert_eq!(rows[1].output, "x\ty.o");
        assert_eq!(rows[1].args, strings(&["-oz\\w", "-"]));
        assert!(parse_manifest("zz\t1\n").is_err());
        assert!(parse_manifest("ab\tx\to\n").is_err());
    }

    #[test]
    fn replay() {
        let (args, at) = replay_args(&strings(&["--strict", "-O0", "-o", "a.o", "-"])).unwrap();
        assert_eq!((args[at].as_str(), at), ("a.o", 3));
        let (args, at) = replay_args(&strings(&["-oa.o", "-"])).unwrap();
        assert_eq!(args, strings(&["-o", "a.o", "-"]));
        assert_eq!(at, 1);
        assert!(replay_args(&strings(&["-o", "a.o", "in.ll"])).is_err());
        assert!(replay_args(&strings(&["-"])).is_err());
    }
}
