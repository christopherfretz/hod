//! The corpus's two tables: `MANIFEST.tsv`, one row per entry, and
//! `SKIPPED.tsv`, one row per test that has no entry and why. Contract:
//! `spec/harness.md` "corpus", "Manifest".
//!
//! Both are plain text, sorted, and hold nothing that changes from one
//! capture of the same inputs to the next (no times, no process ids), so a
//! repeated capture rewrites them byte for byte.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// The file names, in the corpus directory.
pub const MANIFEST: &str = "MANIFEST.tsv";
pub const SKIPPED: &str = "SKIPPED.tsv";

const MANIFEST_HEADER: &str =
    "entry\tsource\tllvm\ttest\trevision\tproducer\tproducer_args\tllc_args\tsha256\tbytes\tkey";
const SKIPPED_HEADER: &str = "source\tllvm\ttest\treason\tdetail\tkey";

/// One entry of the corpus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The entry's path in the corpus, with `/` separators:
    /// `<source>/llvm<N>/<test>.ll`.
    pub entry: String,
    /// `kiln` or `gcc-torture`.
    pub source: String,
    /// The LLVM major the module was written for.
    pub llvm: u32,
    /// The source test, as its suite names it.
    pub test: String,
    /// The revision of the suite the test was read from.
    pub revision: String,
    /// The compiler that wrote the module, and its version.
    pub producer: String,
    /// The arguments the producer was given, without paths of this machine.
    pub producer_args: String,
    /// The arguments `llc` was given for this module, without the output
    /// and the input.
    pub llc_args: String,
    /// SHA-256 of the entry file.
    pub sha256: String,
    /// Size of the entry file.
    pub bytes: u64,
    /// The hash of everything the entry was captured from ("Incremental").
    pub key: String,
}

/// One test that has no entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skip {
    pub source: String,
    pub llvm: u32,
    pub test: String,
    /// A name of `spec/harness.md` "corpus", "Skip reasons".
    pub reason: String,
    /// What exactly happened, one line.
    pub detail: String,
    pub key: String,
}

/// A field of a table: a backslash, tab, newline and carriage return are
/// written `\\`, `\t`, `\n`, `\r`; an empty value is `-`.
pub fn escape(value: &str) -> String {
    if value.is_empty() {
        return "-".to_owned();
    }
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// The value [`escape`] wrote. (A value that is the one character `-`
/// comes back empty; no column holds one.)
pub fn unescape(field: &str) -> String {
    if field == "-" {
        return String::new();
    }
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

/// The identity of a test in the tables: its source, LLVM major and name.
pub type TestKey = (String, u32, String);

/// `MANIFEST.tsv`, by entry path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub rows: BTreeMap<String, Row>,
}

/// `SKIPPED.tsv`, by test.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Skips {
    pub rows: BTreeMap<TestKey, Skip>,
}

/// The lines of a table after its header; a missing file is an empty table.
fn table(path: &Path, header: &str) -> Result<Vec<Vec<String>>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read '{}': {e}", path.display())),
    };
    let mut lines = text.lines();
    if lines.next() != Some(header) {
        return Err(format!("'{}' does not start with the expected header line", path.display()));
    }
    let columns = header.split('\t').count();
    let mut rows = Vec::new();
    for (index, line) in lines.enumerate() {
        let fields: Vec<String> = line.split('\t').map(unescape).collect();
        if fields.len() != columns {
            return Err(format!(
                "'{}' line {}: {} fields, expected {columns}",
                path.display(),
                index + 2,
                fields.len()
            ));
        }
        rows.push(fields);
    }
    Ok(rows)
}

fn number<T: std::str::FromStr>(text: &str, what: &str, path: &Path) -> Result<T, String> {
    text.parse().map_err(|_| format!("'{}': invalid {what} '{text}'", path.display()))
}

/// Writes `text` to `path` through a temporary file beside it, so a reader
/// never sees half a table.
fn replace(path: &Path, text: &str) -> Result<(), String> {
    let temp = path.with_extension("tsv.tmp");
    fs::write(&temp, text).map_err(|e| format!("cannot write '{}': {e}", temp.display()))?;
    fs::rename(&temp, path).map_err(|e| format!("cannot write '{}': {e}", path.display()))
}

impl Manifest {
    /// Reads `<dir>/MANIFEST.tsv`; a missing file is an empty manifest.
    pub fn read(dir: &Path) -> Result<Self, String> {
        let path = dir.join(MANIFEST);
        let mut rows = BTreeMap::new();
        for fields in table(&path, MANIFEST_HEADER)? {
            let [
                entry,
                source,
                llvm,
                test,
                revision,
                producer,
                producer_args,
                llc_args,
                sha256,
                bytes,
                key,
            ] = <[String; 11]>::try_from(fields).map_err(|_| "unreachable field count")?;
            let row = Row {
                llvm: number(&llvm, "LLVM major", &path)?,
                bytes: number(&bytes, "size", &path)?,
                entry: entry.clone(),
                source,
                test,
                revision,
                producer,
                producer_args,
                llc_args,
                sha256,
                key,
            };
            rows.insert(entry, row);
        }
        Ok(Self { rows })
    }

    /// The table's text: the header, then the rows in entry order.
    pub fn text(&self) -> String {
        let mut out = format!("{MANIFEST_HEADER}\n");
        for row in self.rows.values() {
            let fields = [
                escape(&row.entry),
                escape(&row.source),
                row.llvm.to_string(),
                escape(&row.test),
                escape(&row.revision),
                escape(&row.producer),
                escape(&row.producer_args),
                escape(&row.llc_args),
                escape(&row.sha256),
                row.bytes.to_string(),
                escape(&row.key),
            ];
            out.push_str(&fields.join("\t"));
            out.push('\n');
        }
        out
    }

    /// Writes `<dir>/MANIFEST.tsv`.
    pub fn write(&self, dir: &Path) -> Result<(), String> {
        replace(&dir.join(MANIFEST), &self.text())
    }

    /// The row of a test, if it has an entry.
    pub fn of_test(&self, source: &str, llvm: u32, test: &str) -> Option<&Row> {
        self.rows.values().find(|row| row.source == source && row.llvm == llvm && row.test == test)
    }
}

impl Skips {
    /// Reads `<dir>/SKIPPED.tsv`; a missing file is an empty table.
    pub fn read(dir: &Path) -> Result<Self, String> {
        let path = dir.join(SKIPPED);
        let mut rows = BTreeMap::new();
        for fields in table(&path, SKIPPED_HEADER)? {
            let [source, llvm, test, reason, detail, key] =
                <[String; 6]>::try_from(fields).map_err(|_| "unreachable field count")?;
            let llvm: u32 = number(&llvm, "LLVM major", &path)?;
            let skip =
                Skip { source: source.clone(), llvm, test: test.clone(), reason, detail, key };
            rows.insert((source, llvm, test), skip);
        }
        Ok(Self { rows })
    }

    /// The table's text: the header, then the rows in test order.
    pub fn text(&self) -> String {
        let mut out = format!("{SKIPPED_HEADER}\n");
        for skip in self.rows.values() {
            let fields = [
                escape(&skip.source),
                skip.llvm.to_string(),
                escape(&skip.test),
                escape(&skip.reason),
                escape(&skip.detail),
                escape(&skip.key),
            ];
            out.push_str(&fields.join("\t"));
            out.push('\n');
        }
        out
    }

    /// Writes `<dir>/SKIPPED.tsv`.
    pub fn write(&self, dir: &Path) -> Result<(), String> {
        replace(&dir.join(SKIPPED), &self.text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::Scratch;

    fn row(entry: &str) -> Row {
        Row {
            entry: entry.to_owned(),
            source: "kiln".to_owned(),
            llvm: 22,
            test: "tests/a b.cpp".to_owned(),
            revision: "0123abcd".to_owned(),
            producer: "kiln version 1.0.1 (C++26 frontend)".to_owned(),
            producer_args: "-std=c++26 -DX=\"a\tb\"".to_owned(),
            llc_args: "-O0 -relocation-model=pic -filetype=obj".to_owned(),
            sha256: "ab".repeat(32),
            bytes: 1234,
            key: "cd".repeat(32),
        }
    }

    // spec/harness.md "corpus", "Manifest": the escaping of a field.
    #[test]
    fn fields_are_escaped() {
        for value in ["", "plain", "a\tb", "back\\slash", "two\nlines\r", "\\t", "ünï"] {
            let written = escape(value);
            assert!(!written.is_empty() && !written.contains(['\t', '\n', '\r']), "{written:?}");
            assert_eq!(unescape(&written), value);
        }
        assert_eq!(escape(""), "-");
        assert_eq!(escape("a\tb\\"), "a\\tb\\\\");
    }

    // "Manifest": the tables are read back as written, in sorted order,
    // and writing them again changes nothing.
    #[test]
    fn tables_round_trip() {
        let scratch = Scratch::new("corpus-manifest");
        let dir = scratch.path();
        assert_eq!(Manifest::read(dir).unwrap(), Manifest::default());
        assert_eq!(Skips::read(dir).unwrap(), Skips::default());

        let mut manifest = Manifest::default();
        for entry in ["kiln/llvm22/b.cpp.ll", "kiln/llvm22/a.cpp.ll"] {
            manifest.rows.insert(entry.to_owned(), row(entry));
        }
        manifest.write(dir).unwrap();
        let text = fs::read_to_string(dir.join(MANIFEST)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("entry\tsource\tllvm\ttest\trevision\t"));
        assert!(lines[1].starts_with("kiln/llvm22/a.cpp.ll\tkiln\t22\ttests/a b.cpp\t"));
        assert!(lines[1].contains("\t-std=c++26 -DX=\"a\\tb\"\t"), "{}", lines[1]);
        assert!(lines[1].ends_with(&format!("\t1234\t{}", "cd".repeat(32))));
        assert_eq!(Manifest::read(dir).unwrap(), manifest);
        manifest.write(dir).unwrap();
        assert_eq!(fs::read_to_string(dir.join(MANIFEST)).unwrap(), text);
        assert_eq!(manifest.of_test("kiln", 22, "tests/a b.cpp").unwrap().entry, lines[1][..20]);
        assert!(manifest.of_test("kiln", 19, "tests/a b.cpp").is_none());

        let mut skips = Skips::default();
        for (test, reason, detail) in
            [("z.c", "clang-rejects", "error: x\ny"), ("a.c", "multi-file", "")]
        {
            let skip = Skip {
                source: "gcc-torture".to_owned(),
                llvm: 19,
                test: test.to_owned(),
                reason: reason.to_owned(),
                detail: detail.to_owned(),
                key: "k".to_owned(),
            };
            skips.rows.insert(("gcc-torture".to_owned(), 19, test.to_owned()), skip);
        }
        skips.write(dir).unwrap();
        let text = fs::read_to_string(dir.join(SKIPPED)).unwrap();
        assert_eq!(
            text,
            "source\tllvm\ttest\treason\tdetail\tkey\n\
             gcc-torture\t19\ta.c\tmulti-file\t-\tk\n\
             gcc-torture\t19\tz.c\tclang-rejects\terror: x\\ny\tk\n"
        );
        assert_eq!(Skips::read(dir).unwrap(), skips);
    }

    // "Manifest": a damaged table is an error, not an empty corpus.
    #[test]
    fn damaged_tables_are_refused() {
        let scratch = Scratch::new("corpus-manifest-bad");
        let dir = scratch.path();
        fs::write(dir.join(MANIFEST), "something else\n").unwrap();
        assert!(Manifest::read(dir).unwrap_err().contains("expected header"));
        fs::write(dir.join(MANIFEST), format!("{MANIFEST_HEADER}\na\tb\n")).unwrap();
        assert!(Manifest::read(dir).unwrap_err().contains("line 2: 2 fields, expected 11"));
        let mut bad = row("x.ll");
        bad.sha256 = "s".to_owned();
        let line = Manifest { rows: [("x.ll".to_owned(), bad)].into() }.text();
        fs::write(dir.join(MANIFEST), line.replace("\t22\t", "\ttwenty\t")).unwrap();
        assert!(Manifest::read(dir).unwrap_err().contains("invalid LLVM major 'twenty'"));
        fs::write(dir.join(SKIPPED), format!("{SKIPPED_HEADER}\nkiln\t22\n")).unwrap();
        assert!(Skips::read(dir).unwrap_err().contains("line 2: 2 fields, expected 6"));
    }
}
