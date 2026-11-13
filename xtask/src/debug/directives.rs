//! The directives of a debugger test. Contract: `spec/harness.md`
//! "debug", "Debugger tests".
//!
//! A test is a C (`.c`), C++ (`.cpp`) or LLVM IR (`.ll`) file with at
//! least one directive: a line `//@ <key>: <value>` (`;@` in IR). A file
//! without directives is not a test (a companion module, a header).

use std::path::{Path, PathBuf};

/// The source language of a test file, by its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    C,
    Cxx,
    Ir,
}

impl Lang {
    /// The language of `path`, or `None` when it is not a test file.
    pub fn of(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "c" => Some(Self::C),
            "cpp" | "cc" | "cxx" => Some(Self::Cxx),
            "ll" => Some(Self::Ir),
            _ => None,
        }
    }

    /// What a directive line starts with.
    pub fn marker(self) -> &'static str {
        match self {
            Self::C | Self::Cxx => "//@",
            Self::Ir => ";@",
        }
    }
}

/// The front ends a test can be compiled by. `clang-19`, `clang-22` and
/// `kiln` (`kiln++`, C++ only) turn a source into IR; `ir-19` and `ir-22`
/// are an IR test given to that LLVM's `llc` as it is.
pub const FRONTENDS: [&str; 5] = ["clang-19", "clang-22", "kiln", "ir-19", "ir-22"];

/// The debuggers' names in scopes: `gdb`, and `lldb` for every lldb
/// (`lldb-19`, `lldb-22` name one).
pub const DEBUGGER_WORDS: [&str; 4] = ["gdb", "lldb", "lldb-19", "lldb-22"];

/// The linkers' names in scopes.
pub const LINKER_WORDS: [&str; 4] = ["bfd", "lld", "mold", "wild"];

/// The targets' names in scopes and in `target:` (WP-136): the
/// architecture the programs are built for (`--target`).
pub const TARGET_WORDS: [&str; 2] = ["aarch64", "x86_64"];

/// The scope word of a session through an emulator's gdb stub (a target
/// that is not this machine's, WP-136).
pub const REMOTE_WORD: &str = "remote";

/// When an `xfail` applies: each list that is not empty must contain the
/// session's front end, linker or debugger.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub frontends: Vec<String>,
    pub linkers: Vec<String>,
    pub debuggers: Vec<String>,
    /// WP-136: the targets (`aarch64`, `x86_64`).
    pub targets: Vec<String>,
    /// WP-136: `remote`, only sessions through an emulator's gdb stub.
    pub remote: bool,
}

impl Scope {
    /// Whether the scope covers sessions on `target` (an architecture's
    /// name), `remote` when they go through an emulator's gdb stub: when
    /// it names no target, every one; `remote` only the remote ones.
    pub fn covers_target(&self, target: &str, remote: bool) -> bool {
        (self.targets.is_empty() || self.targets.iter().any(|t| t == target))
            && (!self.remote || remote)
    }

    /// Whether the scope covers a session. `debugger` is `gdb` or
    /// `lldb-<N>`, which `lldb` also names.
    pub fn covers(&self, frontend: &str, linker: Option<&str>, debugger: Option<&str>) -> bool {
        let front = self.frontends.is_empty() || self.frontends.iter().any(|f| f == frontend);
        let link =
            self.linkers.is_empty() || linker.is_some_and(|l| self.linkers.iter().any(|w| w == l));
        let debug = self.debuggers.is_empty()
            || debugger.is_some_and(|d| {
                self.debuggers.iter().any(|w| w == d || (w == "lldb" && d.starts_with("lldb")))
            });
        front && link && debug
    }

    fn parse(text: &str) -> Result<Self, String> {
        let mut scope = Self::default();
        for word in text.split([',', ' ']).filter(|w| !w.is_empty()) {
            if FRONTENDS.contains(&word) {
                scope.frontends.push(word.to_owned());
            } else if LINKER_WORDS.contains(&word) {
                scope.linkers.push(word.to_owned());
            } else if DEBUGGER_WORDS.contains(&word) {
                scope.debuggers.push(word.to_owned());
            } else if TARGET_WORDS.contains(&word) {
                scope.targets.push(word.to_owned());
            } else if word == REMOTE_WORD {
                scope.remote = true;
            } else {
                return Err(format!("unknown word '{word}' in an xfail scope"));
            }
        }
        Ok(scope)
    }
}

/// An expected failure: where it applies, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Xfail {
    pub scope: Scope,
    pub reason: String,
}

/// One debugger's script and checks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Script {
    /// The commands, in order.
    pub commands: Vec<String>,
    /// Substrings the transcript must contain, each on a line after the
    /// previous one's.
    pub checks: Vec<String>,
}

/// Which programs a test links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Objects {
    /// Every module by hod, against every module by `llc`.
    Whole,
    /// Also each module by hod with the others by `llc`, in the modules'
    /// order (so with two modules, hod's object first and then `llc`'s).
    Mixed,
}

/// A parsed test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugTest {
    /// The test file.
    pub path: PathBuf,
    pub lang: Lang,
    /// The other modules of the program (`module:`), resolved beside the
    /// test, in order. The test's own file is the first module.
    pub modules: Vec<PathBuf>,
    pub objects: Objects,
    /// `frontend:`; empty is every front end the language allows.
    pub frontends: Vec<String>,
    /// `cflags:`, given to the front end.
    pub cflags: Vec<String>,
    /// `link:`, given to the linker driver after the objects.
    pub link: Vec<String>,
    pub quick: bool,
    /// `pre-opt: checks`: the IR before LLVM's passes, `opt` before
    /// `llc`, the sessions judged by the checks (WP-108).
    pub pre_opt: bool,
    pub gdb: Script,
    pub lldb: Script,
    pub xfails: Vec<Xfail>,
    /// `target:` (WP-136): the targets the test is run for; empty is
    /// every one.
    pub targets: Vec<String>,
    /// `skip(<scope>): <reason>` (WP-136): sessions not run, for what a
    /// debugger cannot do there (a gdb stub's limits).
    pub skips: Vec<Xfail>,
}

impl DebugTest {
    /// Whether the test is run for `target` (an architecture's name).
    pub fn runs_on(&self, target: &str) -> bool {
        self.targets.is_empty() || self.targets.iter().any(|t| t == target)
    }

    /// The first `xfail` that covers a session on `target` (`remote`:
    /// through an emulator's gdb stub).
    pub fn xfail_on(
        &self,
        (target, remote): (&str, bool),
        frontend: &str,
        linker: Option<&str>,
        debugger: Option<&str>,
    ) -> Option<&Xfail> {
        self.xfails.iter().find(|x| {
            x.scope.covers_target(target, remote) && x.scope.covers(frontend, linker, debugger)
        })
    }

    /// The first `skip` that covers a session, like [`Self::xfail_on`].
    pub fn skip_on(
        &self,
        (target, remote): (&str, bool),
        frontend: &str,
        linker: Option<&str>,
        debugger: Option<&str>,
    ) -> Option<&Xfail> {
        self.skips.iter().find(|x| {
            x.scope.covers_target(target, remote) && x.scope.covers(frontend, linker, debugger)
        })
    }

    /// The front ends this test is built by, in [`FRONTENDS`] order.
    pub fn frontends(&self) -> Vec<&'static str> {
        let allowed: &[&str] = match self.lang {
            Lang::C => &["clang-19", "clang-22"],
            Lang::Cxx => &["clang-19", "clang-22", "kiln"],
            Lang::Ir => &["ir-19", "ir-22"],
        };
        allowed
            .iter()
            .copied()
            .filter(|f| self.frontends.is_empty() || self.frontends.iter().any(|w| w == f))
            .collect()
    }

    /// The first `xfail` that covers a session on this machine's
    /// architecture.
    pub fn xfail(
        &self,
        frontend: &str,
        linker: Option<&str>,
        debugger: Option<&str>,
    ) -> Option<&Xfail> {
        let host = crate::irdiff::directives::Arch::host().map_or("", |a| a.name());
        self.xfail_on((host, false), frontend, linker, debugger)
    }
}

/// Whether `text` has a directive line for `lang`.
pub fn has_directives(text: &str, lang: Lang) -> bool {
    text.lines().any(|line| line.trim_start().starts_with(lang.marker()))
}

/// The words of a `target:` directive.
fn target_words(value: &str) -> Result<Vec<String>, String> {
    value
        .split_whitespace()
        .map(|word| {
            if TARGET_WORDS.contains(&word) {
                Ok(word.to_owned())
            } else {
                Err(format!("unknown target '{word}'"))
            }
        })
        .collect()
}

/// An `xfail` or `skip` directive (`key`: `xfail`, `xfail(<scope>)`,
/// `skip`, `skip(<scope>)`; `value`: the reason).
fn scoped(key: &str, value: &str) -> Result<Xfail, String> {
    let word = if key.starts_with("skip") { "skip" } else { "xfail" };
    let scope = match key.strip_prefix(word).unwrap_or("") {
        "" => Scope::default(),
        inner => {
            let inner = inner.strip_prefix('(').and_then(|s| s.strip_suffix(')'));
            Scope::parse(inner.ok_or_else(|| format!("bad {word} scope '{key}'"))?)?
        }
    };
    if value.is_empty() {
        let what = if word == "skip" { "a skip" } else { "an xfail" };
        return Err(format!("{what} needs a reason"));
    }
    Ok(Xfail { scope, reason: value.to_owned() })
}

/// Parses the directives of `path`, whose content is `source`. An error
/// is a broken test.
pub fn parse(path: &Path, source: &str) -> Result<DebugTest, String> {
    let lang = Lang::of(path).ok_or_else(|| format!("'{}' is not a test file", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut test = DebugTest {
        path: path.to_path_buf(),
        lang,
        modules: Vec::new(),
        objects: Objects::Whole,
        frontends: Vec::new(),
        cflags: Vec::new(),
        link: Vec::new(),
        quick: false,
        pre_opt: false,
        gdb: Script::default(),
        lldb: Script::default(),
        xfails: Vec::new(),
        targets: Vec::new(),
        skips: Vec::new(),
    };
    for (number, line) in source.lines().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix(lang.marker()) else { continue };
        let at = |message: String| format!("line {}: {message}", number + 1);
        let (key, value) =
            rest.split_once(':').ok_or_else(|| at(format!("no ':' in '{}'", rest.trim())))?;
        let (key, value) = (key.trim(), value.trim());
        let words = || value.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        match key {
            "gdb" => test.gdb.commands.push(value.to_owned()),
            "lldb" => test.lldb.commands.push(value.to_owned()),
            "gdb-check" => test.gdb.checks.push(value.to_owned()),
            "lldb-check" => test.lldb.checks.push(value.to_owned()),
            "module" => {
                let module = dir.join(value);
                if Lang::of(&module) != Some(lang) || !module.is_file() {
                    return Err(at(format!("module '{value}' is not a file of the same language")));
                }
                test.modules.push(module);
            }
            "objects" => {
                test.objects = match value {
                    "whole" => Objects::Whole,
                    "mixed" => Objects::Mixed,
                    _ => return Err(at(format!("objects: '{value}' is not whole or mixed"))),
                };
            }
            "frontend" => {
                for word in words() {
                    if !FRONTENDS.contains(&word.as_str()) {
                        return Err(at(format!("unknown front end '{word}'")));
                    }
                    test.frontends.push(word);
                }
            }
            "target" => test.targets.extend(target_words(value).map_err(at)?),
            "cflags" => test.cflags.extend(words()),
            "link" => test.link.extend(words()),
            "tier" => {
                test.quick = match value {
                    "quick" => true,
                    "all" => false,
                    _ => return Err(at(format!("tier: '{value}' is not quick or all"))),
                };
            }
            "pre-opt" => {
                if value != "checks" {
                    return Err(at(format!("pre-opt: '{value}' is not checks")));
                }
                test.pre_opt = true;
            }
            _ if key == "skip"
                || key.starts_with("skip(")
                || key == "xfail"
                || key.starts_with("xfail(") =>
            {
                let skip = key.starts_with("skip");
                let scoped = scoped(key, value).map_err(at)?;
                if skip { test.skips.push(scoped) } else { test.xfails.push(scoped) }
            }
            _ => return Err(at(format!("unknown directive '{key}'"))),
        }
    }
    for (name, script) in [("gdb", &test.gdb), ("lldb", &test.lldb)] {
        if script.commands.is_empty() && !script.checks.is_empty() {
            return Err(format!("{name}-check without {name} commands"));
        }
    }
    if test.gdb.commands.is_empty() && test.lldb.commands.is_empty() {
        return Err("no gdb or lldb commands".to_owned());
    }
    if test.objects == Objects::Mixed && test.modules.is_empty() {
        return Err("objects: mixed needs a second module".to_owned());
    }
    if test.frontends().is_empty() {
        return Err("no front end of the test's language is allowed".to_owned());
    }
    test.modules.insert(0, path.to_path_buf());
    Ok(test)
}

/// The test files under `paths` (files, or directories searched
/// recursively): files of a test language with directives, sorted, without
/// duplicates. An explicitly named file is a test even without directives
/// (and then fails to parse).
pub fn discover(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for path in paths {
        if path.is_dir() {
            walk(path, &mut found)?;
        } else if path.is_file() {
            found.push(path.clone());
        } else {
            return Err(format!("no such test or directory: '{}'", path.display()));
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("cannot read '{}': {e}", dir.display()))?.path();
        if path.is_dir() {
            walk(&path, found)?;
        } else if let Some(lang) = Lang::of(&path) {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if has_directives(&text, lang) {
                found.push(path);
            }
        }
    }
    Ok(())
}

/// The first check of `checks` not found, in order, on successive lines of
/// `text`: each check must be on a line after the previous check's line.
pub fn first_missing_check<'a>(text: &str, checks: &'a [String]) -> Option<&'a str> {
    let mut lines = text.lines();
    checks
        .iter()
        .find(|check| !lines.by_ref().any(|line| line.contains(check.as_str())))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::Scratch;

    fn parsed(text: &str) -> Result<DebugTest, String> {
        parse(Path::new("t.cpp"), text)
    }

    // spec/harness.md "debug", "Debugger tests": every key.
    #[test]
    fn reads_every_directive() {
        let scratch = Scratch::new("debug-directives");
        let main = scratch.join("a.cpp");
        std::fs::write(scratch.join("b.cpp"), "int b;\n").unwrap();
        let source = "//@ gdb: break main\n//@ gdb: run\n//@ gdb-check: Breakpoint 1\n\
                    //@ lldb: run\n//@ lldb-check: stop reason\n//@ module: b.cpp\n\
                    //@ objects: mixed\n//@ frontend: clang-22 kiln\n//@ cflags: -std=c++20 -DX\n\
                    //@ link: -pthread\n//@ tier: quick\n//@ pre-opt: checks\n//@ xfail(kiln, lldb): kiln: a reason\n\
                    //@ xfail: everywhere\nint main() {}\n";
        std::fs::write(&main, source).unwrap();
        let test = parse(&main, source).unwrap();
        assert_eq!(test.lang, Lang::Cxx);
        assert_eq!(test.modules, vec![main.clone(), scratch.join("b.cpp")]);
        assert_eq!(test.objects, Objects::Mixed);
        assert_eq!(test.frontends(), vec!["clang-22", "kiln"]);
        assert_eq!(test.cflags, vec!["-std=c++20", "-DX"]);
        assert_eq!(test.link, vec!["-pthread"]);
        assert!(test.quick);
        assert!(test.pre_opt);
        assert_eq!(test.gdb.commands, vec!["break main", "run"]);
        assert_eq!(test.gdb.checks, vec!["Breakpoint 1"]);
        assert_eq!(test.lldb.checks, vec!["stop reason"]);
        let first = test.xfail("kiln", Some("bfd"), Some("lldb-19")).unwrap();
        assert_eq!(first.reason, "kiln: a reason");
        assert_eq!(test.xfail("clang-22", Some("bfd"), Some("gdb")).unwrap().reason, "everywhere");
    }

    // "Debugger tests": what makes a test broken.
    #[test]
    fn refuses_broken_tests() {
        let refused = |text: &str, wanted: &str| {
            let error = parsed(text).unwrap_err();
            assert!(error.contains(wanted), "{text:?}: {error}");
        };
        refused("//@ gdb-check: x\n//@ lldb: run\n", "gdb-check without gdb commands");
        refused("//@ tier: quick\n", "no gdb or lldb commands");
        refused("//@ gdb: run\n//@ frob: x\n", "line 2: unknown directive 'frob'");
        refused("//@ gdb: run\n//@ objects: mixed\n", "needs a second module");
        refused("//@ gdb: run\n//@ module: nowhere.cpp\n", "is not a file");
        refused("//@ gdb: run\n//@ xfail(gold): x\n", "unknown word 'gold'");
        refused("//@ gdb: run\n//@ xfail:\n", "needs a reason");
        refused("//@ gdb: run\n//@ frontend: gcc\n", "unknown front end 'gcc'");
        refused("//@ gdb run\n", "no ':'");
        refused("//@ gdb: run\n//@ pre-opt: yes\n", "is not checks");
        let error = parse(Path::new("t.c"), "//@ gdb: run\n//@ frontend: kiln\n").unwrap_err();
        assert!(error.contains("no front end"), "{error}");
    }

    // "Debugger tests": the scope of an xfail.
    #[test]
    fn scopes() {
        let scope = Scope::parse("lldb, mold kiln clang-19").unwrap();
        assert!(scope.covers("kiln", Some("mold"), Some("lldb-19")));
        assert!(scope.covers("clang-19", Some("mold"), Some("lldb-22")));
        assert!(!scope.covers("clang-22", Some("mold"), Some("lldb-19")));
        assert!(!scope.covers("kiln", Some("bfd"), Some("lldb-19")));
        assert!(!scope.covers("kiln", Some("mold"), Some("gdb")));
        // A session decided before any linker or debugger ran is covered
        // only by a scope that names none.
        assert!(!scope.covers("kiln", None, None));
        assert!(Scope::default().covers("kiln", None, None));
        assert!(Scope::parse("lldb-19").unwrap().covers("ir-19", Some("bfd"), Some("lldb-19")));
        assert!(!Scope::parse("lldb-19").unwrap().covers("ir-19", Some("bfd"), Some("lldb-22")));
    }

    // "Debugger tests": checks match in order, each on a later line.
    #[test]
    fn checks_in_order() {
        let checks = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let text = "a 1\nb 2\nc 3\n";
        assert_eq!(first_missing_check(text, &checks(&["a", "c"])), None);
        assert_eq!(first_missing_check(text, &checks(&["c", "a"])), Some("a"));
        assert_eq!(first_missing_check(text, &checks(&["a", "a"])), Some("a"));
        assert_eq!(first_missing_check(text, &checks(&["1", "b", "3"])), None);
    }

    // "Debugger tests": discovery skips files without directives.
    #[test]
    fn discovery() {
        let scratch = Scratch::new("debug-discover");
        std::fs::create_dir_all(scratch.join("sub")).unwrap();
        std::fs::write(scratch.join("t.c"), "//@ gdb: run\n").unwrap();
        std::fs::write(scratch.join("sub/u.ll"), ";@ gdb: run\n").unwrap();
        std::fs::write(scratch.join("helper.cpp"), "int x;\n").unwrap();
        std::fs::write(scratch.join("notes.txt"), "//@ gdb: run\n").unwrap();
        let found = discover(&[scratch.path().to_path_buf()]).unwrap();
        assert_eq!(found, vec![scratch.join("sub/u.ll"), scratch.join("t.c")]);
        assert!(discover(&[scratch.join("missing")]).is_err());
    }
}
