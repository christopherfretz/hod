//! A corpus entry: an `irdiff` test made of `;@` directives, three comment
//! lines that say where the module came from, and the module as its
//! producer wrote it. Contract: `spec/harness.md` "corpus", "Entries".

use crate::irdiff::directives::{self, Arch, Exit, Expectations, Lib, LlvmReq};
use crate::irdiff::process::{End, Output};
use std::fmt::Write as _;

/// The first of the comment lines between the directives and the module.
pub const BANNER: &str = "; corpus entry written by `cargo xtask corpus capture`; do not edit";

/// The most standard output an entry's directives may hold.
pub const STDOUT_CAP: usize = 256 << 10;

/// Why something that was captured cannot be an entry: a skip reason of
/// `spec/harness.md` "corpus" and what exactly is in the way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unfit {
    pub reason: &'static str,
    pub detail: String,
}

impl Unfit {
    pub fn new(reason: &'static str, detail: impl Into<String>) -> Self {
        Self { reason, detail: detail.into() }
    }
}

/// How the program of a captured module is linked and run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipe {
    /// What the entry covers: `<source> <test>`.
    pub covers: String,
    pub arch: Arch,
    /// The LLVM major the module was written for.
    pub llvm: u32,
    pub libs: Vec<Lib>,
    pub pie: bool,
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
}

/// Text that a directive value can hold: lines of UTF-8, the last one
/// ended, without the characters that end an IR comment early.
fn directive_text(bytes: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "it is not UTF-8".to_owned())?;
    if !text.is_empty() && !text.ends_with('\n') {
        return Err("it does not end with a newline".to_owned());
    }
    if text.contains(['\r', '\0']) {
        return Err("it has a carriage return or a NUL".to_owned());
    }
    Ok(())
}

/// The expectations of an entry whose program, built from the module by
/// the real `llc`, did `run`. Only a program that exited is an entry.
pub fn expectations(recipe: &Recipe, run: &Output) -> Result<Expectations, Unfit> {
    let code = match run.end {
        End::Exit(code) => u8::try_from(code)
            .map_err(|_| Unfit::new("program-fails", format!("exit status {code}")))?,
        End::Signal(signal) => {
            return Err(Unfit::new("program-fails", format!("terminated by signal {signal}")));
        }
        End::TimedOut => return Err(Unfit::new("program-fails", "timed out")),
    };
    if run.truncated || run.stdout.len() > STDOUT_CAP {
        let detail = format!("more than {STDOUT_CAP} bytes of standard output");
        return Err(Unfit::new("stdout-not-expressible", detail));
    }
    directive_text(&run.stdout)
        .map_err(|why| Unfit::new("stdout-not-expressible", format!("standard output: {why}")))?;
    directive_text(&recipe.stdin)
        .map_err(|why| Unfit::new("stdin-not-expressible", format!("standard input: {why}")))?;
    if let Some(arg) = recipe.args.iter().find(|arg| {
        arg.is_empty() || arg.contains(|c: char| c.is_whitespace() || c == '\0' || c.is_control())
    }) {
        let detail = format!("argument {arg:?} is empty or has white space");
        return Err(Unfit::new("args-not-expressible", detail));
    }
    let expect = Expectations {
        stdout: run.stdout.clone(),
        stdin: recipe.stdin.clone(),
        exit: Exit::Code(code),
        args: recipe.args.clone(),
        targets: vec![recipe.arch],
        llvm: vec![LlvmReq::Exactly(recipe.llvm)],
        libs: recipe.libs.clone(),
        pie: recipe.pie,
        covers: Some(recipe.covers.clone()),
        ..Expectations::default()
    };
    // What is written must read back as itself; anything the checks above
    // let through and the format cannot hold stops here.
    match directives::parse(&expect.to_directives()) {
        Ok(read) if read == expect => Ok(expect),
        _ => Err(Unfit::new(
            "stdout-not-expressible",
            "the expectations do not survive being written as directives",
        )),
    }
}

/// The entry file: the directives, the provenance comments, the module.
/// The module is not changed in any way.
pub fn compose(
    expect: &Expectations,
    producer_args: &str,
    llc_args: &str,
    module: &[u8],
) -> Result<String, Unfit> {
    let module = std::str::from_utf8(module)
        .map_err(|_| Unfit::new("module-not-text", "the module is not UTF-8 text"))?;
    if module.split('\n').any(|line| line.starts_with(";@")) {
        return Err(Unfit::new("module-not-text", "the module has a line that starts with ';@'"));
    }
    if module.contains('\0') {
        return Err(Unfit::new("module-not-text", "the module has a NUL"));
    }
    let covers = expect.covers.as_deref().unwrap_or("-");
    let one_line = |text: &str| text.replace(['\n', '\r'], " ");
    let mut text = expect.to_directives();
    text.push_str(BANNER);
    text.push('\n');
    let _ = writeln!(text, "; source: {}", one_line(covers));
    let _ = writeln!(text, "; producer-args: {}", one_line(producer_args));
    let _ = writeln!(text, "; llc-args: {}", one_line(llc_args));
    text.push_str(module);
    Ok(text)
}

/// The module of an entry: what follows its `; llc-args:` line. A file
/// without the header is all module.
pub fn module_of(entry: &str) -> &str {
    let mut offset = 0;
    let mut seen_banner = false;
    for line in entry.split_inclusive('\n') {
        let text = line.strip_suffix('\n').unwrap_or(line);
        offset += line.len();
        if !seen_banner {
            if text == BANNER {
                seen_banner = true;
            } else if !text.starts_with(";@") {
                break;
            }
        } else if text.starts_with("; llc-args:") {
            return &entry[offset..];
        } else if !text.starts_with("; ") {
            break;
        }
    }
    entry
}

/// The value of the entry's `; <key>: <value>` provenance line.
pub fn provenance<'a>(entry: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("; {key}: ");
    let header = &entry[..entry.len() - module_of(entry).len()];
    header.lines().find_map(|line| line.strip_prefix(prefix.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe() -> Recipe {
        Recipe {
            covers: "kiln tests/conformance/a/b.cpp".to_owned(),
            arch: Arch::Aarch64,
            llvm: 22,
            libs: vec![Lib::Cxx, Lib::M],
            pie: true,
            args: Vec::new(),
            stdin: Vec::new(),
        }
    }

    fn ran(end: End, stdout: &[u8]) -> Output {
        Output { end, stdout: stdout.to_vec(), stderr: Vec::new(), truncated: false }
    }

    // spec/harness.md "corpus", "Entries": the directives of an entry, in
    // the canonical order, and the file around them.
    #[test]
    fn directives_of_an_entry() {
        let mut recipe = recipe();
        recipe.args = vec!["one".to_owned(), "--two".to_owned()];
        recipe.stdin = b"3 4\n".to_vec();
        recipe.pie = false;
        let expect = expectations(&recipe, &ran(End::Exit(7), b"sum 7\n\n  indented\n")).unwrap();
        let module = "; ModuleID = 'a'\ndefine i32 @main() {\n  ret i32 7\n}\n";
        let text = compose(
            &expect,
            "-std=c++26 tests/conformance/a/b.cpp",
            "-O0 -relocation-model=static -filetype=obj",
            module.as_bytes(),
        )
        .unwrap();
        assert_eq!(
            text,
            ";@ covers: kiln tests/conformance/a/b.cpp\n\
             ;@ target: aarch64\n\
             ;@ llvm: 22\n\
             ;@ link: c++ m no-pie\n\
             ;@ args: one --two\n\
             ;@ stdin: 3 4\n\
             ;@ exit: 7\n\
             ;@ stdout: sum 7\n\
             ;@ stdout:\n\
             ;@ stdout:   indented\n\
             ; corpus entry written by `cargo xtask corpus capture`; do not edit\n\
             ; source: kiln tests/conformance/a/b.cpp\n\
             ; producer-args: -std=c++26 tests/conformance/a/b.cpp\n\
             ; llc-args: -O0 -relocation-model=static -filetype=obj\n\
             ; ModuleID = 'a'\n\
             define i32 @main() {\n  ret i32 7\n}\n"
        );
        // The harness reads back what was meant, and the module is intact.
        assert_eq!(directives::parse(&text).unwrap(), expect);
        assert_eq!(module_of(&text), module);
        assert_eq!(
            provenance(&text, "llc-args"),
            Some("-O0 -relocation-model=static -filetype=obj")
        );
        assert_eq!(provenance(&text, "source"), Some("kiln tests/conformance/a/b.cpp"));
        assert_eq!(provenance(&text, "nothing"), None);

        // The defaults are left out: a C program that prints nothing.
        let quiet = Recipe { libs: vec![Lib::M], ..self::recipe() };
        let expect = expectations(&quiet, &ran(End::Exit(0), b"")).unwrap();
        assert_eq!(
            expect.to_directives(),
            ";@ covers: kiln tests/conformance/a/b.cpp\n;@ target: aarch64\n;@ llvm: 22\n;@ link: m\n"
        );
    }

    // "Entries": a file that is not an entry is all module.
    #[test]
    fn module_of_other_files() {
        assert_eq!(module_of(""), "");
        assert_eq!(module_of("define void @f() {\n}\n"), "define void @f() {\n}\n");
        let hand = ";@ exit: 3\n; a comment\ndefine i32 @main() {\n}\n";
        assert_eq!(module_of(hand), hand);
        let no_module = format!(";@ exit: 3\n{BANNER}\n; source: x\n; llc-args: -O0\n");
        assert_eq!(module_of(&no_module), "");
    }

    // "Skip reasons": what the directive format cannot hold is not an
    // entry, and neither is a program that did not exit.
    #[test]
    fn unfit_runs_and_expectations() {
        let reason = |recipe: &Recipe, run: &Output| expectations(recipe, run).unwrap_err().reason;
        let plain = recipe();
        assert_eq!(reason(&plain, &ran(End::Signal(11), b"")), "program-fails");
        assert_eq!(reason(&plain, &ran(End::TimedOut, b"")), "program-fails");
        assert_eq!(reason(&plain, &ran(End::Exit(0), b"no newline")), "stdout-not-expressible");
        assert_eq!(reason(&plain, &ran(End::Exit(0), b"\xff\n")), "stdout-not-expressible");
        assert_eq!(reason(&plain, &ran(End::Exit(0), b"a\r\n")), "stdout-not-expressible");
        assert_eq!(reason(&plain, &ran(End::Exit(0), b"a\0b\n")), "stdout-not-expressible");
        let big = vec![b'\n'; STDOUT_CAP + 1];
        assert_eq!(reason(&plain, &ran(End::Exit(0), &big)), "stdout-not-expressible");
        let mut truncated = ran(End::Exit(0), b"x\n");
        truncated.truncated = true;
        assert_eq!(reason(&plain, &truncated), "stdout-not-expressible");

        let spaced = Recipe { args: vec!["a b".to_owned()], ..recipe() };
        assert_eq!(reason(&spaced, &ran(End::Exit(0), b"")), "args-not-expressible");
        let empty = Recipe { args: vec![String::new()], ..recipe() };
        assert_eq!(reason(&empty, &ran(End::Exit(0), b"")), "args-not-expressible");
        let input = Recipe { stdin: b"unterminated".to_vec(), ..recipe() };
        assert_eq!(reason(&input, &ran(End::Exit(0), b"")), "stdin-not-expressible");

        let expect = expectations(&plain, &ran(End::Exit(0), b"")).unwrap();
        let unfit = |module: &[u8]| compose(&expect, "", "", module).unwrap_err().reason;
        assert_eq!(unfit(b"BC\xc0\xde"), "module-not-text");
        assert_eq!(unfit(b"define void @f() {\n;@ exit: 1\n}\n"), "module-not-text");
        assert_eq!(unfit(b"a\0b"), "module-not-text");
    }
}
