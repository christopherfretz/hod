//! A corpus entry: an `irdiff` test made of `;@` directives, comment lines
//! that say where the modules came from, and each module as its producer
//! wrote it. Contract: `spec/harness.md` "corpus", "Entries".

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
    /// The arguments of the producer's `llc` run that the entry's
    /// `llc-args` directive repeats.
    pub llc_args: Vec<String>,
}

/// How a module reached `llc`: as text, or as bitcode that the capture
/// wrote as text with `llvm-dis`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivered {
    Text,
    Bitcode,
}

impl Delivered {
    pub fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Bitcode => "bitcode",
        }
    }
}

/// One module of an entry: its name (none for an entry of one module), its
/// text, and how it reached `llc`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub name: Option<String>,
    pub text: Vec<u8>,
    pub delivered: Delivered,
}

/// What the manifest and the entry say of how the modules were delivered:
/// one word per module, in order.
pub fn delivered(modules: &[Module]) -> String {
    let words: Vec<&str> = modules.iter().map(|module| module.delivered.name()).collect();
    words.join(" ")
}

/// Text that a directive value can hold: lines of UTF-8 without the
/// characters that end an IR comment early; the last line ended, unless
/// `open_end`.
fn directive_text(bytes: &[u8], open_end: bool) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "it is not UTF-8".to_owned())?;
    if !open_end && !text.is_empty() && !text.ends_with('\n') {
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
    // Output without a final newline is a `stdout-no-newline` line.
    directive_text(&run.stdout, true)
        .map_err(|why| Unfit::new("stdout-not-expressible", format!("standard output: {why}")))?;
    directive_text(&recipe.stdin, false)
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
        llc_args: recipe.llc_args.clone(),
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

/// The entry file: the directives, the provenance comments, the modules.
/// A module's text is not changed in any way; a named module follows its
/// `;@ module:` line, and one that does not end with a newline gets one.
pub fn compose(
    expect: &Expectations,
    producer_args: &str,
    llc_args: &str,
    modules: &[Module],
) -> Result<String, Unfit> {
    let covers = expect.covers.as_deref().unwrap_or("-");
    let one_line = |text: &str| text.replace(['\n', '\r'], " ");
    let mut text = expect.to_directives();
    text.push_str(BANNER);
    text.push('\n');
    let _ = writeln!(text, "; source: {}", one_line(covers));
    let _ = writeln!(text, "; producer-args: {}", one_line(producer_args));
    let _ = writeln!(text, "; delivered: {}", delivered(modules));
    let _ = writeln!(text, "; llc-args: {}", one_line(llc_args));
    let named = modules.iter().any(|module| module.name.is_some());
    if modules.is_empty() || (named && modules.iter().any(|module| module.name.is_none())) {
        return Err(Unfit::new("capture-broken", "an entry is one module, or named modules"));
    }
    for module in modules {
        let unfit = |what: &str| {
            let of = module.name.as_ref().map_or_else(String::new, |name| format!(" '{name}'"));
            Unfit::new("module-not-text", format!("the module{of} {what}"))
        };
        let code = std::str::from_utf8(&module.text).map_err(|_| unfit("is not UTF-8 text"))?;
        if code.split('\n').any(|line| line.starts_with(";@")) {
            return Err(unfit("has a line that starts with ';@'"));
        }
        if code.contains('\0') {
            return Err(unfit("has a NUL"));
        }
        if let Some(name) = &module.name {
            let _ = writeln!(text, ";@ module: {name}");
        }
        text.push_str(code);
        if named && !code.ends_with('\n') {
            text.push('\n');
        }
    }
    // The file must read back as the entry it is meant to be.
    let given: Vec<&str> = modules.iter().filter_map(|module| module.name.as_deref()).collect();
    let read: Vec<String> =
        directives::modules(&text).into_iter().map(|module| module.name).collect();
    match directives::parse(&text) {
        Ok(found) if found == *expect && read == given => Ok(text),
        _ => Err(Unfit::new("capture-broken", "the entry does not read back as written")),
    }
}

/// The modules of an entry: what follows its `; llc-args:` line (for an
/// entry of several modules, from the first `;@ module:` line on). A file
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
            llc_args: Vec::new(),
        }
    }

    fn one(text: &[u8]) -> Vec<Module> {
        vec![Module { name: None, text: text.to_vec(), delivered: Delivered::Text }]
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
            &one(module.as_bytes()),
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
             ; delivered: text\n\
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

    // "Entries": the `llc-args` directive, output without a final newline,
    // a module that was bitcode, and an entry of several modules.
    #[test]
    fn directives_and_modules_of_an_entry() {
        let mut recipe = recipe();
        recipe.llc_args = vec!["-debugger-tune=gdb".to_owned()];
        let expect = expectations(&recipe, &ran(End::Exit(0), b"a\nno newline")).unwrap();
        assert_eq!(expect.stdout, b"a\nno newline");
        let mut modules = one(b"@x = global i32 0\n");
        modules[0].delivered = Delivered::Bitcode;
        let text = compose(&expect, "-O2 t.cpp", "-O2 -debugger-tune=gdb", &modules).unwrap();
        assert_eq!(
            text,
            ";@ covers: kiln tests/conformance/a/b.cpp\n\
             ;@ target: aarch64\n\
             ;@ llvm: 22\n\
             ;@ link: c++ m\n\
             ;@ llc-args: -debugger-tune=gdb\n\
             ;@ stdout: a\n\
             ;@ stdout-no-newline: no newline\n\
             ; corpus entry written by `cargo xtask corpus capture`; do not edit\n\
             ; source: kiln tests/conformance/a/b.cpp\n\
             ; producer-args: -O2 t.cpp\n\
             ; delivered: bitcode\n\
             ; llc-args: -O2 -debugger-tune=gdb\n\
             @x = global i32 0\n"
        );
        assert_eq!(provenance(&text, "delivered"), Some("bitcode"));

        let module = |name: &str, text: &str, delivered| Module {
            name: Some(name.to_owned()),
            text: text.as_bytes().to_vec(),
            delivered,
        };
        let expect = expectations(&self::recipe(), &ran(End::Exit(0), b"")).unwrap();
        let several = [
            module("m.part", "@a = global i32 0\n", Delivered::Text),
            module("test", "@b = global i32 0", Delivered::Bitcode),
        ];
        let text = compose(&expect, "a && b", "-O0", &several).unwrap();
        assert!(
            text.ends_with(
                "; delivered: text bitcode\n; llc-args: -O0\n\
                 ;@ module: m.part\n@a = global i32 0\n;@ module: test\n@b = global i32 0\n"
            ),
            "{text}"
        );
        assert_eq!(directives::parse(&text).unwrap(), expect);
        let read = directives::modules(&text);
        assert_eq!(read.len(), 2);
        assert!(read[1].text.ends_with(";@ module: test\n@b = global i32 0\n"));
        assert!(module_of(&text).starts_with(";@ module: m.part\n"));
        assert_eq!(delivered(&several[..1]), "text");
        // Named and unnamed modules do not mix, and names are not repeated.
        let mixed = [several[0].clone(), one(b"")[0].clone()];
        assert_eq!(compose(&expect, "", "", &mixed).unwrap_err().reason, "capture-broken");
        let twice = [several[0].clone(), several[0].clone()];
        assert_eq!(compose(&expect, "", "", &twice).unwrap_err().reason, "capture-broken");
        assert_eq!(compose(&expect, "", "", &[]).unwrap_err().reason, "capture-broken");
    }

    // "Skip reasons": what the directive format cannot hold is not an
    // entry, and neither is a program that did not exit.
    #[test]
    fn unfit_runs_and_expectations() {
        let reason = |recipe: &Recipe, run: &Output| expectations(recipe, run).unwrap_err().reason;
        let plain = recipe();
        assert_eq!(reason(&plain, &ran(End::Signal(11), b"")), "program-fails");
        assert_eq!(reason(&plain, &ran(End::TimedOut, b"")), "program-fails");
        // Output without a final newline is expressible, white space too.
        for stdout in [&b"no newline"[..], b"a\n ", b" \tx "] {
            assert_eq!(expectations(&plain, &ran(End::Exit(0), stdout)).unwrap().stdout, stdout);
        }
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
        let unfit = |module: &[u8]| compose(&expect, "", "", &one(module)).unwrap_err().reason;
        assert_eq!(unfit(b"BC\xc0\xde"), "module-not-text");
        assert_eq!(unfit(b"define void @f() {\n;@ exit: 1\n}\n"), "module-not-text");
        assert_eq!(unfit(b"a\0b"), "module-not-text");
    }
}
