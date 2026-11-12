//! kiln's executing tests as a source of IR. Contract: `spec/harness.md`
//! "corpus", "The kiln source".
//!
//! A kiln test is a C or C++ file whose leading `//` comment lines carry
//! `//@` directives (kiln `spec/harness.md`; `xtask/src/test/directives.rs`
//! and `xtask/src/util/header.rs` are what this module follows). The tests
//! with `expect: run` are compiled to a program and run. This module finds
//! them, reads the part of the header that decides how such a test is
//! built and run, builds kiln's command line as kiln's own harness does,
//! and names what does not fit a corpus entry.

use std::fs;
use std::path::{Path, PathBuf};

use super::entry::Unfit;
use crate::irdiff::directives::Arch;

/// The test roots of kiln's default run that hold `run` tests, relative to
/// the repository.
pub const ROOTS: [&str; 3] = ["tests/c", "tests/conformance", "tests/stress"];

/// The script that stands where hod stands in a shim (`spec/harness.md`
/// "shim"): it saves the arguments and the standard input kiln gives its
/// backend into a new directory `llc-<pid>` of `$HOD_CORPUS_CAPTURE`, then
/// runs the real `llc` (the shim's `$HOD_FALLBACK_LLC`) on what it saved,
/// so that kiln's build completes exactly as without it.
pub const CAPTURE_SCRIPT: &str = "#!/bin/sh\n\
# Written by `cargo xtask corpus capture` (spec/harness.md \"corpus\").\n\
# Saves what kiln gives its backend, then runs the real llc on it.\n\
call=\"${HOD_CORPUS_CAPTURE:?}/llc-$$\"\n\
mkdir \"$call\" || exit 97\n\
for arg in \"$@\"; do printf '%s\\n' \"$arg\"; done > \"$call/args\" || exit 97\n\
cat > \"$call/stdin\" || exit 97\n\
exec \"${HOD_FALLBACK_LLC:?}\" \"$@\" < \"$call/stdin\"\n";

/// The source language of a test, from its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    C,
    Cxx,
}

/// One test file of kiln's corpus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KilnTest {
    /// The path relative to the repository, with `/`: the test's name, and
    /// what kiln is given (it runs in the repository's root).
    pub name: String,
    pub path: PathBuf,
    pub lang: Lang,
    /// A `.h` test: compiled with `-x c++`.
    pub is_header: bool,
}

/// What a `run` test's header says about building and running it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    /// `expect:`.
    pub expect: Option<String>,
    /// `std:`.
    pub std: Option<String>,
    /// `args:`, every line's words.
    pub args: Vec<String>,
    /// The `stdout:` lines, each ended by a newline.
    pub stdout: Option<String>,
    /// `stdout-file:`, relative to the test's directory.
    pub stdout_file: Option<String>,
    /// `exit:`.
    pub exit: Option<u8>,
    /// `run-args:`.
    pub run_args: Vec<String>,
    /// The `run-stdin:` lines, each ended by a newline.
    pub stdin: Option<String>,
    /// `run-stdin-file:`, relative to the test's directory.
    pub stdin_file: Option<String>,
    /// The keys, each once, of the directives that make a test something
    /// other than one translation unit built by one command.
    pub multi_file: Vec<String>,
    /// `env:` is present.
    pub env: bool,
    /// The reason of a `skip` that applies to kiln.
    pub skip: Option<String>,
    /// The reason of an `xfail` that applies to kiln.
    pub xfail: Option<String>,
    /// The `requires:` values, and `min-oracle: N` as `clang>=N`.
    pub requires: Vec<String>,
    /// What could not be read.
    pub errors: Vec<String>,
}

/// kiln's `split_args`: words separated by white space; `'...'` verbatim;
/// `"..."` with `\"` and `\\`; a backslash takes the next character.
pub fn split_args(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("unterminated ' in arguments".to_owned()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => word.push(c),
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err("unterminated \" in arguments".to_owned()),
                        },
                        Some(c) => word.push(c),
                        None => return Err("unterminated \" in arguments".to_owned()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some(c) => word.push(c),
                    None => return Err("trailing \\ in arguments".to_owned()),
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        out.push(word);
    }
    Ok(out)
}

/// Reads the `//@` directives of `source`'s header: the leading run of
/// lines that are blank or `//` comments.
pub fn parse_header(source: &str) -> Header {
    let mut header = Header::default();
    for raw in source.lines() {
        let text = raw.trim();
        if !(text.is_empty() || text.starts_with("//")) {
            break;
        }
        let Some(body) = text.strip_prefix("//@") else { continue };
        let (key, value) = match body.split_once(':') {
            Some((key, value)) => (key.trim(), value.trim()),
            None => (body.trim(), ""),
        };
        // `skip(kiln)`, `xfail(clang)`: the compiler a marker is for.
        let (name, qualifier) = match key.split_once('(') {
            Some((name, rest)) => (name, rest.strip_suffix(')')),
            None => (key, None),
        };
        let for_kiln = matches!(qualifier, None | Some("kiln"));
        let words = |into: &mut Vec<String>, errors: &mut Vec<String>| match split_args(value) {
            Ok(mut words) => into.append(&mut words),
            Err(e) => errors.push(format!("{name}: {e}")),
        };
        match name {
            "expect" => header.expect = Some(value.to_owned()),
            "std" => header.std = Some(value.to_owned()),
            "args" => words(&mut header.args, &mut header.errors),
            "run-args" => words(&mut header.run_args, &mut header.errors),
            "stdout" => {
                let lines = header.stdout.get_or_insert_with(String::new);
                lines.push_str(value);
                lines.push('\n');
            }
            "stdout-file" => header.stdout_file = Some(value.to_owned()),
            "run-stdin" => {
                let lines = header.stdin.get_or_insert_with(String::new);
                lines.push_str(value);
                lines.push('\n');
            }
            "run-stdin-file" => header.stdin_file = Some(value.to_owned()),
            "exit" => match value.parse::<u8>() {
                Ok(code) => header.exit = Some(code),
                Err(_) => header.errors.push(format!("exit: invalid status '{value}'")),
            },
            "env" => header.env = true,
            "skip" if for_kiln => header.skip = Some(value.to_owned()),
            "xfail" if for_kiln => header.xfail = Some(value.to_owned()),
            "module" | "unit" | "header-unit" | "pch" | "two-step" => {
                let seen = header.multi_file.iter().any(|seen| seen == name);
                header.multi_file.extend((!seen).then(|| name.to_owned()));
            }
            "requires" => header.requires.push(value.to_owned()),
            "min-oracle" => header.requires.push(format!("clang>={value}")),
            _ => {}
        }
    }
    header
}

/// Finds the test files under the [`ROOTS`] of the kiln repository `repo`,
/// as kiln's harness does: `*.c` and `*.cpp`, and a `*.h` that has a
/// directive header; no name that starts with `.`, nothing under an
/// `Inputs` directory. Sorted by name.
pub fn discover(repo: &Path) -> Result<Vec<KilnTest>, String> {
    let mut found = Vec::new();
    for root in ROOTS {
        let dir = repo.join(root);
        if dir.is_dir() {
            walk(&dir, root, &mut found)?;
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

fn walk(dir: &Path, name: &str, found: &mut Vec<KilnTest>) -> Result<(), String> {
    let failed = |e: std::io::Error| format!("cannot read '{}': {e}", dir.display());
    for entry in fs::read_dir(dir).map_err(failed)? {
        let entry = entry.map_err(failed)?;
        let file = entry.file_name().to_string_lossy().into_owned();
        if file.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let child = format!("{name}/{file}");
        if path.is_dir() {
            if file != "Inputs" {
                walk(&path, &child, found)?;
            }
            continue;
        }
        let extension = file.rsplit_once('.').map(|(_, extension)| extension);
        let is_test = match extension {
            Some("c" | "cpp") => true,
            Some("h") => fs::read(&path).is_ok_and(|bytes| {
                String::from_utf8_lossy(&bytes)
                    .lines()
                    .map(str::trim)
                    .take_while(|line| line.is_empty() || line.starts_with("//"))
                    .any(|line| line.starts_with("//@"))
            }),
            _ => false,
        };
        if is_test {
            found.push(KilnTest {
                name: child,
                path,
                lang: if extension == Some("c") { Lang::C } else { Lang::Cxx },
                is_header: extension == Some("h"),
            });
        }
    }
    Ok(())
}

/// The architecture a triple names, in this harness's spelling.
fn triple_arch(triple: &str) -> &str {
    match triple.split('-').next().unwrap_or(triple) {
        "arm64" => "aarch64",
        "amd64" => "x86_64",
        arch => arch,
    }
}

/// The `--target` a test's arguments pin: the last of `--target=<t>`,
/// `--target <t>` and `-target <t>`.
fn target_triple(args: &[String]) -> Option<&str> {
    let mut found = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if let Some(triple) = arg.strip_prefix("--target=") {
            found = Some(triple);
        } else if arg == "--target" || arg == "-target" {
            found = args.next().map(String::as_str).or(found);
        }
    }
    found
}

/// The leading integer of a version requirement (`22`, `19.1.7`).
fn leading_major(text: &str) -> Option<u32> {
    let digits: String = text.trim().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// A `%{name}` of a test's arguments: `test_dir` and `repo_root` are
/// written relative to the repository's root, where kiln runs; a name that
/// needs a toolchain this capture does not have is `Err(name)`.
fn substitute(arg: &str, test_dir: &str) -> Result<String, String> {
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(at) = rest.find("%{") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let Some(end) = after.find('}') else {
            out.push_str("%{");
            rest = after;
            continue;
        };
        match &after[..end] {
            "test_dir" => out.push_str(test_dir),
            "repo_root" => out.push('.'),
            "resource_dir" => out.push_str("resource"),
            other => return Err(other.to_owned()),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The arguments of a test that only a link understands and that the
/// harness's own link (`spec/harness.md` "irdiff") cannot repeat.
fn link_only(arg: &str) -> bool {
    (arg.starts_with("-l") && arg != "-lm" && arg != "-lstdc++")
        || arg.starts_with("-L")
        || arg.starts_with("-Wl,")
        || arg.starts_with("-fuse-ld")
        || arg.starts_with("-static")
        || matches!(arg, "-shared" | "-rdynamic" | "-nostdlib" | "-nodefaultlibs" | "-nostartfiles")
}

/// How to build one test, and how its program is to be run and checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// kiln's arguments after `--llvm-dir=` and before `-o <program>`: as
    /// kiln's harness passes them, the test by its name.
    pub compile_args: Vec<String>,
    pub run_args: Vec<String>,
    pub stdin: Vec<u8>,
    /// What the test itself expects of its program.
    pub want_stdout: Option<String>,
    pub want_exit: u8,
}

/// Decides, before anything is run, whether `test` can be a corpus entry
/// for a capture on `host` with LLVM `major`, and how it is built.
pub fn plan(test: &KilnTest, header: &Header, host: Arch, major: u32) -> Result<Plan, Unfit> {
    let skip = |reason: &'static str, detail: String| Err(Unfit::new(reason, detail));
    if let Some(error) = header.errors.first() {
        return skip("header-error", error.clone());
    }
    if let Some(why) = &header.skip {
        return skip("marked-skip", why.clone());
    }
    if let Some(why) = &header.xfail {
        return skip("marked-xfail", why.clone());
    }
    if !header.multi_file.is_empty() {
        return skip("multi-file", format!("directives: {}", header.multi_file.join(", ")));
    }
    if header.env {
        return skip("needs-env", "the test sets environment variables (env:)".to_owned());
    }
    let arch = target_triple(&header.args).map_or(host.name(), triple_arch);
    if arch != host.name() {
        return skip("other-target", format!("the test targets {arch}"));
    }
    for requirement in &header.requires {
        let unmet = if let Some(wanted) = requirement.strip_prefix("target-") {
            wanted != arch
        } else if requirement.starts_with("host-") || requirement == "sanitizer-runtime" {
            // A program that needs a CPU feature of the capturing machine,
            // or a sanitizer runtime, is not a portable entry.
            true
        } else if let Some(version) =
            requirement.strip_prefix("clang>=").or_else(|| requirement.strip_prefix("llvm>="))
        {
            leading_major(version).is_none_or(|wanted| major < wanted)
        } else if let Some(version) =
            requirement.strip_prefix("clang<").or_else(|| requirement.strip_prefix("llvm<"))
        {
            leading_major(version).is_none_or(|wanted| major >= wanted)
        } else {
            true
        };
        if unmet {
            return skip("requires", format!("requires: {requirement}"));
        }
    }
    let test_dir = test.name.rsplit_once('/').map_or(".", |(dir, _)| dir);
    let mut args = Vec::new();
    for arg in &header.args {
        match substitute(arg, test_dir) {
            Ok(arg) => args.push(arg),
            Err(name) => return skip("needs-toolchain", format!("args use %{{{name}}}")),
        }
    }
    if let Some(arg) = args.iter().find(|arg| arg.starts_with("-stdlib=libc++")) {
        return skip("libc++", format!("{arg}: the harness links libstdc++"));
    }
    if let Some(arg) = args.iter().find(|arg| link_only(arg)) {
        return skip("link-flags", format!("{arg}: the harness's link cannot repeat it"));
    }

    let mut compile_args = Vec::new();
    match (test.lang, test.is_header) {
        (Lang::C, _) => compile_args.extend(["-x".to_owned(), "c".to_owned()]),
        (Lang::Cxx, true) => compile_args.extend(["-x".to_owned(), "c++".to_owned()]),
        (Lang::Cxx, false) => {}
    }
    let default_std = if test.lang == Lang::C { "c17" } else { "c++26" };
    compile_args.push(format!("-std={}", header.std.as_deref().unwrap_or(default_std)));
    compile_args.push("-fno-crash-diagnostics".to_owned());
    compile_args.push(format!("-fclang-compat={major}"));
    compile_args.extend(args);
    compile_args.push(test.name.clone());

    let beside = |file: &str| test.path.parent().unwrap_or(Path::new(".")).join(file);
    let read = |file: &str| {
        fs::read(beside(file)).map_err(|e| Unfit::new("header-error", format!("{file}: {e}")))
    };
    let stdin = match (&header.stdin, &header.stdin_file) {
        (Some(lines), _) => lines.clone().into_bytes(),
        (None, Some(file)) => read(file)?,
        (None, None) => Vec::new(),
    };
    let want_stdout = match (&header.stdout, &header.stdout_file) {
        (Some(lines), _) => Some(lines.clone()),
        (None, Some(file)) => Some(String::from_utf8_lossy(&read(file)?).into_owned()),
        (None, None) => None,
    };
    Ok(Plan {
        compile_args,
        run_args: header.run_args.clone(),
        stdin,
        want_stdout,
        want_exit: header.exit.unwrap_or(0),
    })
}

/// What kiln gave its backend in one `llc` run, as the capture script
/// saved it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendCall {
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
}

/// Reads the `llc-*` directories the capture script left in `dir`.
pub fn backend_calls(dir: &Path) -> Result<Vec<BackendCall>, String> {
    let failed = |e: std::io::Error| format!("cannot read '{}': {e}", dir.display());
    let mut calls = Vec::new();
    let mut names: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir).map_err(failed)? {
        let path = entry.map_err(failed)?.path();
        if path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("llc-")) {
            names.push(path);
        }
    }
    names.sort();
    for call in names {
        let args = fs::read_to_string(call.join("args")).map_err(failed)?;
        let stdin = fs::read(call.join("stdin")).map_err(failed)?;
        calls.push(BackendCall { args: args.lines().map(str::to_owned).collect(), stdin });
    }
    Ok(calls)
}

/// Whether `module` is LLVM bitcode (or its wrapper), as hod tells
/// (`spec/cli.md` "Input").
pub fn is_bitcode(module: &[u8]) -> bool {
    module.starts_with(&[0x42, 0x43, 0xC0, 0xDE]) || module.starts_with(&[0xDE, 0xC0, 0x17, 0x0B])
}

/// The text of a test and of the files it includes by `#include "..."`,
/// found beside the including file, three levels deep: where the
/// constructs that send a module through `opt` are looked for.
pub fn source_with_includes(path: &Path) -> String {
    fn add(path: &Path, depth: u32, seen: &mut Vec<PathBuf>, text: &mut String) {
        if seen.iter().any(|done| done == path) {
            return;
        }
        seen.push(path.to_path_buf());
        let Ok(bytes) = fs::read(path) else { return };
        let source = String::from_utf8_lossy(&bytes).into_owned();
        if depth < 3 {
            for line in source.lines() {
                let Some(rest) = line.trim_start().strip_prefix('#') else { continue };
                let Some(rest) = rest.trim_start().strip_prefix("include") else { continue };
                let Some(rest) = rest.trim_start().strip_prefix('"') else { continue };
                if let Some((file, _)) = rest.split_once('"') {
                    let beside = path.parent().unwrap_or(Path::new(".")).join(file);
                    add(&beside, depth + 1, seen, text);
                }
            }
        }
        text.push_str(&source);
        text.push('\n');
    }
    let mut text = String::new();
    add(path, 0, &mut Vec::new(), &mut text);
    text
}

/// Why kiln ran `opt` before `llc` for a test (kiln
/// `crates/kiln_driver/src/codegen_cmd.rs`, `ir_passes`), told from the
/// test's arguments and its source text ([`source_with_includes`]): the
/// skip reason. A guess where the arguments do not say: a coroutine
/// keyword or header in the source is `opt-first-coroutine`; anything
/// else is `opt-first-always-inline`, the one other cause a test without
/// special arguments has (an `always_inline` function, most often of a
/// standard library header).
pub fn opt_first_reason(compile_args: &[String], source: &str) -> &'static str {
    let has = |prefix: &str| compile_args.iter().any(|arg| arg.starts_with(prefix));
    let level = compile_args
        .iter()
        .rev()
        .find(|arg| arg.starts_with("-O") && !arg.starts_with("-Ob"))
        .map(String::as_str);
    if has("-flto") {
        "opt-first-lto"
    } else if has("-fsanitize=") {
        "opt-first-sanitizer"
    } else if has("--coverage")
        || has("-fprofile-instr-generate")
        || has("-fprofile-arcs")
        || has("-ftest-coverage")
        || has("-fcoverage-mapping")
    {
        "opt-first-coverage"
    } else if level.is_some_and(|level| level != "-O0") {
        "opt-first-optimized"
    } else if ["co_await", "co_yield", "co_return", "<coroutine>", "<generator>"]
        .iter()
        .any(|mark| source.contains(mark))
    {
        "opt-first-coroutine"
    } else {
        "opt-first-always-inline"
    }
}

/// The arguments of one `llc` run without its output and its input:
/// `-o <object>` and the final `-` are dropped. `None` if the command line
/// is not the shape kiln's driver writes (`... -o <object> -`).
pub fn llc_args(args: &[String]) -> Option<Vec<String>> {
    let (last, rest) = args.split_last()?;
    let at = rest.iter().position(|arg| arg == "-o")?;
    if last != "-" || at + 1 >= rest.len() {
        return None;
    }
    let mut kept: Vec<String> = rest[..at].to_vec();
    kept.extend_from_slice(&rest[at + 2..]);
    Some(kept)
}

/// The arguments `llc` gets from `irdiff` for a module with relocation
/// model `model`, in kiln's order (`spec/harness.md` "irdiff", "What a
/// test does"; the triple is the module's own).
pub fn harness_llc_args(pie: bool) -> [&'static str; 3] {
    ["-O0", if pie { "-relocation-model=pic" } else { "-relocation-model=static" }, "-filetype=obj"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::Scratch;

    fn test(name: &str) -> KilnTest {
        let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
        let lang = if extension == "c" { Lang::C } else { Lang::Cxx };
        KilnTest {
            name: name.to_owned(),
            path: PathBuf::from("/kiln").join(name),
            lang,
            is_header: extension == "h",
        }
    }

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    // spec/harness.md "corpus", "The kiln source": the capture script is
    // what a shim runs in hod's place.
    #[test]
    fn capture_script_saves_and_forwards() {
        let scratch = Scratch::new("corpus-capture-script");
        let script = scratch.join("capture-llc");
        hod_testutil::write_script(&script, CAPTURE_SCRIPT.strip_prefix("#!/bin/sh\n").unwrap());
        let real = scratch.write_script(
            "real-llc",
            "printf 'real:'; for a in \"$@\"; do printf ' [%s]' \"$a\"; done; echo; cat; exit 5",
        );
        let captures = scratch.join("captures");
        fs::create_dir(&captures).unwrap();
        let mut command = std::process::Command::new(&script);
        command
            .args(["-O0", "-o", "/tmp/a b.o", "-"])
            .env("HOD_CORPUS_CAPTURE", &captures)
            .env("HOD_FALLBACK_LLC", &real);
        let outcome = hod_testutil::run_with(
            &mut command,
            b"define void @f()\n\x00\xff",
            hod_testutil::DEFAULT_TIMEOUT,
        );
        // The real llc saw the same arguments and the same input, and its
        // status and output are the script's.
        assert_eq!(outcome.status.and_then(|status| status.code()), Some(5));
        assert_eq!(
            outcome.stdout,
            b"real: [-O0] [-o] [/tmp/a b.o] [-]\ndefine void @f()\n\x00\xff"
        );
        let calls = backend_calls(&captures).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].args, ["-O0", "-o", "/tmp/a b.o", "-"]);
        assert_eq!(calls[0].stdin, b"define void @f()\n\x00\xff");
        assert_eq!(llc_args(&calls[0].args).unwrap(), ["-O0"]);

        // Without a capture directory it fails instead of running anything.
        let mut bare = std::process::Command::new(&script);
        bare.env_remove("HOD_CORPUS_CAPTURE").env("HOD_FALLBACK_LLC", &real);
        let outcome = hod_testutil::run_with(&mut bare, b"", hod_testutil::DEFAULT_TIMEOUT);
        assert!(!outcome.success() && outcome.stdout.is_empty());
        // The script itself is plain `sh` with the two variables only.
        assert!(CAPTURE_SCRIPT.starts_with("#!/bin/sh\n"));
        assert_eq!(CAPTURE_SCRIPT.matches("HOD_").count(), 2);
    }

    // "The kiln source": the header of a test.
    #[test]
    fn header_directives() {
        let header = parse_header(
            "// A comment.\n\
             //@ expect: run\n\
             //@ std: c++20\n\
             //@ args: -DA='b c' -I %{test_dir}/Inputs\n\
             //@ args: -O0\n\
             \n\
             //@ stdout: one\n\
             //@ stdout:\n\
             //@ stdout:   two  \n\
             //@ exit: 3\n\
             //@ run-args: x \"y z\"\n\
             //@ run-stdin: in\n\
             //@ skip(clang): not for kiln\n\
             //@ xfail(gcc): nor this\n\
             //@ requires: target-aarch64\n\
             //@ min-oracle: 21\n\
             //@ diag: 3:1 warning \"w\"\n\
             int main() {}\n\
             //@ exit: 9\n",
        );
        assert_eq!(header.expect.as_deref(), Some("run"));
        assert_eq!(header.std.as_deref(), Some("c++20"));
        assert_eq!(header.args, ["-DA=b c", "-I", "%{test_dir}/Inputs", "-O0"]);
        assert_eq!(header.stdout.as_deref(), Some("one\n\ntwo\n"));
        assert_eq!(header.exit, Some(3));
        assert_eq!(header.run_args, ["x", "y z"]);
        assert_eq!(header.stdin.as_deref(), Some("in\n"));
        assert_eq!((header.skip, header.xfail), (None, None));
        assert_eq!(header.requires, ["target-aarch64", "clang>=21"]);
        assert!(header.multi_file.is_empty() && !header.env && header.errors.is_empty());

        let header = parse_header(
            "//@ expect: run\n//@ skip: broken\n//@ xfail(kiln): known\n//@ env: A=b\n\
             //@ module: m=Inputs/m.cppm\n//@ module: n=Inputs/n.cppm\n//@ unit: u.cpp\n\
             //@ two-step\n//@ args: 'open\n//@ exit: 300\n//@ stdout-file: out.txt\n\
             //@ run-stdin-file: in.txt\n",
        );
        assert_eq!(header.skip.as_deref(), Some("broken"));
        assert_eq!(header.xfail.as_deref(), Some("known"));
        assert!(header.env);
        assert_eq!(header.multi_file, ["module", "unit", "two-step"]);
        assert_eq!(header.errors.len(), 2, "{:?}", header.errors);
        assert_eq!(header.stdout_file.as_deref(), Some("out.txt"));
        assert_eq!(header.stdin_file.as_deref(), Some("in.txt"));
        assert_eq!(parse_header("int main() {}\n//@ expect: run\n").expect, None);
    }

    // "The kiln source": kiln's command line for a test.
    #[test]
    fn command_lines() {
        let header = parse_header("//@ expect: run\n//@ stdout: hi\n");
        let c_test = plan(&test("tests/c/hello/h.c"), &header, Arch::Aarch64, 22).unwrap();
        assert_eq!(
            c_test.compile_args,
            [
                "-x",
                "c",
                "-std=c17",
                "-fno-crash-diagnostics",
                "-fclang-compat=22",
                "tests/c/hello/h.c"
            ]
        );
        assert_eq!((c_test.want_stdout.as_deref(), c_test.want_exit), (Some("hi\n"), 0));
        assert!(c_test.run_args.is_empty() && c_test.stdin.is_empty());

        let header = parse_header(
            "//@ expect: run\n//@ std: c++23\n//@ exit: 4\n//@ run-args: a b\n//@ run-stdin: 1 2\n\
             //@ args: -g -I %{test_dir}/Inputs -I%{repo_root}/tests/support -isystem %{resource_dir}\n",
        );
        let cxx_test =
            plan(&test("tests/conformance/x.y/t.cpp"), &header, Arch::X86_64, 19).unwrap();
        assert_eq!(
            cxx_test.compile_args,
            [
                "-std=c++23",
                "-fno-crash-diagnostics",
                "-fclang-compat=19",
                "-g",
                "-I",
                "tests/conformance/x.y/Inputs",
                "-I./tests/support",
                "-isystem",
                "resource",
                "tests/conformance/x.y/t.cpp"
            ]
        );
        assert_eq!((cxx_test.want_stdout, cxx_test.want_exit), (None, 4));
        assert_eq!((cxx_test.run_args, cxx_test.stdin), (strings(&["a", "b"]), b"1 2\n".to_vec()));
        let header_test = plan(
            &test("tests/conformance/h/t.h"),
            &parse_header("//@ expect: run\n"),
            Arch::Aarch64,
            22,
        )
        .unwrap();
        assert_eq!(header_test.compile_args[..3], ["-x", "c++", "-std=c++26"]);
    }

    // "Skip reasons": what is decided from the header alone.
    #[test]
    fn static_skips() {
        let reason = |header: &str, host: Arch, major: u32| {
            let text = format!("//@ expect: run\n{header}");
            plan(&test("tests/conformance/a/t.cpp"), &parse_header(&text), host, major)
                .map_or_else(|unfit| unfit.reason, |_| "captured")
        };
        let on_arm = |header: &str| reason(header, Arch::Aarch64, 22);
        assert_eq!(on_arm(""), "captured");
        assert_eq!(on_arm("//@ skip: no\n"), "marked-skip");
        assert_eq!(on_arm("//@ skip(kiln): no\n"), "marked-skip");
        assert_eq!(on_arm("//@ skip(clang): no\n"), "captured");
        assert_eq!(on_arm("//@ xfail(kiln): no\n"), "marked-xfail");
        assert_eq!(on_arm("//@ module: m=m.cppm\n"), "multi-file");
        assert_eq!(on_arm("//@ unit: u.cpp\n"), "multi-file");
        assert_eq!(on_arm("//@ header-unit: <vector>\n"), "multi-file");
        assert_eq!(on_arm("//@ pch: p.h\n"), "multi-file");
        assert_eq!(on_arm("//@ env: TZ=UTC\n"), "needs-env");
        assert_eq!(on_arm("//@ args: 'x\n"), "header-error");
        assert_eq!(on_arm("//@ args: --target=x86_64-unknown-linux-gnu\n"), "other-target");
        assert_eq!(on_arm("//@ args: --target=aarch64-unknown-linux-gnu\n"), "captured");
        assert_eq!(on_arm("//@ args: -target arm64-apple-macos\n"), "captured");
        assert_eq!(on_arm("//@ requires: target-x86_64\n"), "requires");
        assert_eq!(on_arm("//@ requires: target-aarch64\n"), "captured");
        assert_eq!(reason("//@ requires: target-x86_64\n", Arch::X86_64, 22), "captured");
        assert_eq!(on_arm("//@ requires: host-sve\n"), "requires");
        assert_eq!(on_arm("//@ requires: sanitizer-runtime\n"), "requires");
        assert_eq!(on_arm("//@ requires: clang>=22\n"), "captured");
        assert_eq!(on_arm("//@ requires: clang>=23.1\n"), "requires");
        assert_eq!(on_arm("//@ requires: clang<22\n"), "requires");
        assert_eq!(reason("//@ requires: llvm<22\n", Arch::Aarch64, 19), "captured");
        assert_eq!(reason("//@ requires: llvm>=22\n", Arch::Aarch64, 19), "requires");
        assert_eq!(reason("//@ min-oracle: 21\n", Arch::Aarch64, 19), "requires");
        assert_eq!(on_arm("//@ min-oracle: 21\n"), "captured");
        assert_eq!(on_arm("//@ args: --gcc-toolchain=%{gcc_toolchain}\n"), "needs-toolchain");
        assert_eq!(on_arm("//@ args: -stdlib=libc++\n"), "libc++");
        assert_eq!(on_arm("//@ args: -latomic\n"), "link-flags");
        assert_eq!(on_arm("//@ args: -fuse-ld=lld\n"), "link-flags");
        assert_eq!(on_arm("//@ args: -lm -pthread\n"), "captured");
        assert_eq!(on_arm("//@ stdout-file: missing.txt\n"), "header-error");
    }

    // "The kiln source": which files are tests.
    #[test]
    fn discovery() {
        let scratch = Scratch::new("corpus-kiln-discover");
        for (file, text) in [
            ("tests/conformance/b/t.cpp", "//@ expect: run\n"),
            ("tests/conformance/a/t.cpp", ""),
            ("tests/conformance/a/Inputs/aux.cpp", ""),
            ("tests/conformance/a/with.h", "// x\n//@ expect: run\n"),
            ("tests/conformance/a/plain.h", "int x;\n"),
            ("tests/conformance/a/.hidden.cpp", ""),
            ("tests/conformance/a/notes.txt", ""),
            ("tests/c/run/x.c", ""),
            ("tests/codegen/not-a-root.cpp", ""),
        ] {
            let path = scratch.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let found = discover(scratch.path()).unwrap();
        let names: Vec<&str> = found.iter().map(|test| test.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "tests/c/run/x.c",
                "tests/conformance/a/t.cpp",
                "tests/conformance/a/with.h",
                "tests/conformance/b/t.cpp"
            ]
        );
        assert_eq!(found[0].lang, Lang::C);
        assert!(found[2].is_header && found[2].lang == Lang::Cxx);
        assert_eq!(found[3].path, scratch.join("tests/conformance/b/t.cpp"));
    }

    // "Skip reasons": the text a cause of `opt` is looked for in.
    #[test]
    fn included_text() {
        let scratch = Scratch::new("corpus-kiln-includes");
        fs::create_dir_all(scratch.join("Inputs")).unwrap();
        scratch.write("t.cpp", "#include \"Inputs/a.h\"\n  #  include \"missing.h\"\n#include <vector>\nint main() {}\n");
        scratch.write("Inputs/a.h", "#include \"b.h\"\n#include \"a.h\"\nint a;\n");
        scratch.write("Inputs/b.h", "task f() { co_return; }\n");
        let text = source_with_includes(&scratch.join("t.cpp"));
        assert!(
            text.contains("int main()") && text.contains("int a;") && text.contains("co_return")
        );
        assert_eq!(text.matches("int a;").count(), 1);
        assert_eq!(opt_first_reason(&[], &text), "opt-first-coroutine");
        assert_eq!(source_with_includes(&scratch.join("none.cpp")), "");
    }

    // "The kiln source": what kiln gave llc, and why `opt` ran first.
    #[test]
    fn backend_command_lines() {
        let args = strings(&[
            "-O0",
            "-relocation-model=pic",
            "-filetype=obj",
            "-debugger-tune=gdb",
            "-o",
            "/tmp/kiln-1/0-a.o",
            "-",
        ]);
        assert_eq!(
            llc_args(&args).unwrap(),
            ["-O0", "-relocation-model=pic", "-filetype=obj", "-debugger-tune=gdb"]
        );
        assert_eq!(llc_args(&strings(&["-O0", "-o", "a.o", "a.ll"])), None);
        assert_eq!(llc_args(&strings(&["-O0", "-"])), None);
        assert_eq!(llc_args(&strings(&["-O0", "-o", "-"])), None);
        assert_eq!(llc_args(&[]), None);
        assert_eq!(harness_llc_args(true), ["-O0", "-relocation-model=pic", "-filetype=obj"]);
        assert_eq!(harness_llc_args(false)[1], "-relocation-model=static");

        assert!(is_bitcode(b"BC\xc0\xde rest") && is_bitcode(&[0xDE, 0xC0, 0x17, 0x0B, 0]));
        assert!(!is_bitcode(b"; ModuleID") && !is_bitcode(b""));

        let why = |args: &[&str], source: &str| opt_first_reason(&strings(args), source);
        assert_eq!(why(&["-std=c++26"], "task f() { co_return; }"), "opt-first-coroutine");
        assert_eq!(
            why(&["-std=c++26"], "[[gnu::always_inline]] int f();"),
            "opt-first-always-inline"
        );
        assert_eq!(why(&["-O2"], "co_await x;"), "opt-first-optimized");
        assert_eq!(why(&["-O2", "-O0"], ""), "opt-first-always-inline");
        assert_eq!(why(&["-O0", "-Os"], ""), "opt-first-optimized");
        assert_eq!(why(&["-fsanitize=undefined", "-O1"], ""), "opt-first-sanitizer");
        assert_eq!(why(&["--coverage"], ""), "opt-first-coverage");
        assert_eq!(why(&["-flto=thin", "-fsanitize=cfi"], ""), "opt-first-lto");
    }
}
