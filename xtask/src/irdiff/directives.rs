//! The `;@` directives of an IR test. Contract: `spec/harness.md` "Test
//! directives".
//!
//! A test is an LLVM IR file whose `;@` lines say how its program must
//! behave and where the test applies. [`parse`] reads them into
//! [`Expectations`]; [`Expectations::to_directives`] writes them back in a
//! canonical order. Nothing here touches the file system: a caller that
//! produces IR itself builds an [`Expectations`] directly.

use std::fmt;
use std::fmt::Write as _;

/// A target architecture the harness knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Arch {
    Aarch64,
    X86_64,
}

impl Arch {
    /// Every architecture, in the order of the documentation.
    pub const ALL: [Self; 2] = [Self::Aarch64, Self::X86_64];

    /// The name used in directives and on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Self::Aarch64 => "aarch64",
            Self::X86_64 => "x86_64",
        }
    }

    /// The architecture with this name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|arch| arch.name() == name)
    }

    /// The triple the harness passes as `-mtriple`.
    pub fn triple(self) -> &'static str {
        match self {
            Self::Aarch64 => "aarch64-unknown-linux-gnu",
            Self::X86_64 => "x86_64-unknown-linux-gnu",
        }
    }

    /// The architecture of this machine, if it is a Linux host of a known
    /// architecture: the only kind of host that can link and run a test.
    pub fn host() -> Option<Self> {
        if std::env::consts::OS == "linux" { Self::from_name(std::env::consts::ARCH) } else { None }
    }
}

/// The linker names of the matrix, in report order.
pub const LINKER_NAMES: [&str; 4] = ["bfd", "lld", "mold", "wild"];

/// Which set a test belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tier {
    /// Run by `ci --quick` and by the full gate.
    Quick,
    /// Run by the full gate only.
    #[default]
    Full,
}

/// How a program must end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// It exits with this status.
    Code(u8),
    /// It is terminated by this signal.
    Signal(u8),
}

impl Default for Exit {
    fn default() -> Self {
        Self::Code(0)
    }
}

impl fmt::Display for Exit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Code(code) => write!(f, "{code}"),
            Self::Signal(signal) => write!(f, "signal {signal}"),
        }
    }
}

/// One item of an `llvm:` directive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LlvmReq {
    /// `N`: exactly this major.
    Exactly(u32),
    /// `N+`: this major or a later one.
    AtLeast(u32),
}

impl LlvmReq {
    fn admits(self, major: u32) -> bool {
        match self {
            Self::Exactly(n) => major == n,
            Self::AtLeast(n) => major >= n,
        }
    }
}

/// A library a test links besides the C library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lib {
    /// `m`: the math library.
    M,
    /// `c++`: the C++ runtime (exceptions, RTTI).
    Cxx,
}

/// One condition of an `xfail(...)` or `unsupported(...)` directive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cond {
    /// The target is this architecture.
    Arch(Arch),
    /// The program is linked by this linker (a name of [`LINKER_NAMES`]).
    Linker(String),
    /// The oracle is this LLVM major (`llvm<N>`).
    Llvm(u32),
}

impl Cond {
    fn holds(&self, arch: Arch, llvm: u32, linker: Option<&str>) -> bool {
        match self {
            Self::Arch(wanted) => *wanted == arch,
            Self::Linker(wanted) => linker == Some(wanted.as_str()),
            Self::Llvm(wanted) => *wanted == llvm,
        }
    }
}

impl fmt::Display for Cond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arch(arch) => f.write_str(arch.name()),
            Self::Linker(name) => f.write_str(name),
            Self::Llvm(major) => write!(f, "llvm{major}"),
        }
    }
}

/// An `xfail` or `unsupported` directive: where it applies, and its reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conditional {
    /// All of these must hold; none means everywhere.
    pub when: Vec<Cond>,
    /// `xfail`: why the test fails. `unsupported`: the coverage-log reason
    /// hod must decline with.
    pub reason: String,
}

impl Conditional {
    /// A directive that applies everywhere.
    pub fn always(reason: &str) -> Self {
        Self { when: Vec::new(), reason: reason.to_owned() }
    }

    fn applies(&self, arch: Arch, llvm: u32, linker: Option<&str>) -> bool {
        self.when.iter().all(|cond| cond.holds(arch, llvm, linker))
    }
}

/// Everything the directives of one test say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expectations {
    /// What the program must write to standard output.
    pub stdout: Vec<u8>,
    /// What the program is given on standard input.
    pub stdin: Vec<u8>,
    /// How the program must end.
    pub exit: Exit,
    /// The program's arguments.
    pub args: Vec<String>,
    /// The architectures the test is valid for; empty means all.
    pub targets: Vec<Arch>,
    /// The LLVM majors the test is valid for; empty means all.
    pub llvm: Vec<LlvmReq>,
    /// Libraries to link besides the C library.
    pub libs: Vec<Lib>,
    /// Position-independent executable (the default) or not.
    pub pie: bool,
    /// The set the test belongs to.
    pub tier: Tier,
    /// Where the test is expected to fail, and why.
    pub xfail: Vec<Conditional>,
    /// Where hod is expected to decline the module, and with which reason.
    pub unsupported: Vec<Conditional>,
    /// The spec section the test covers.
    pub covers: Option<String>,
}

impl Default for Expectations {
    fn default() -> Self {
        Self {
            stdout: Vec::new(),
            stdin: Vec::new(),
            exit: Exit::default(),
            args: Vec::new(),
            targets: Vec::new(),
            llvm: Vec::new(),
            libs: Vec::new(),
            pie: true,
            tier: Tier::default(),
            xfail: Vec::new(),
            unsupported: Vec::new(),
            covers: None,
        }
    }
}

impl Expectations {
    /// Whether the test is valid for this architecture.
    pub fn runs_on(&self, arch: Arch) -> bool {
        self.targets.is_empty() || self.targets.contains(&arch)
    }

    /// Whether the test is valid for this LLVM major.
    pub fn accepts_llvm(&self, major: u32) -> bool {
        self.llvm.is_empty() || self.llvm.iter().any(|req| req.admits(major))
    }

    /// The reason hod must decline with here, if an `unsupported`
    /// directive applies: the first that does, in file order.
    pub fn unsupported_reason(&self, arch: Arch, llvm: u32) -> Option<&str> {
        self.unsupported
            .iter()
            .find(|directive| directive.applies(arch, llvm, None))
            .map(|directive| directive.reason.as_str())
    }

    /// Why the test is expected to fail here, if an `xfail` directive
    /// applies: the first that does, in file order. With no linker (the
    /// program is not linked), a directive conditional on a linker does not
    /// apply.
    pub fn xfail_reason(&self, arch: Arch, llvm: u32, linker: Option<&str>) -> Option<&str> {
        self.xfail
            .iter()
            .find(|directive| directive.applies(arch, llvm, linker))
            .map(|directive| directive.reason.as_str())
    }

    /// The directives that say this, in the canonical order of
    /// `spec/harness.md`. Parsing the result gives `self` back, provided
    /// the text fields hold what directives can express: `stdout` and
    /// `stdin` are UTF-8 lines each ended by a newline, and no argument,
    /// reason or `covers` text is empty or has surrounding white space.
    pub fn to_directives(&self) -> String {
        let mut out = String::new();
        let mut line = |key: &str, value: &str| {
            let _ = if value.is_empty() {
                writeln!(out, ";@ {key}:")
            } else {
                writeln!(out, ";@ {key}: {value}")
            };
        };
        if let Some(covers) = &self.covers {
            line("covers", covers);
        }
        if self.tier == Tier::Quick {
            line("tier", "quick");
        }
        if !self.targets.is_empty() {
            line("target", &join(self.targets.iter().map(|arch| arch.name().to_owned())));
        }
        if !self.llvm.is_empty() {
            line(
                "llvm",
                &join(self.llvm.iter().map(|req| match req {
                    LlvmReq::Exactly(n) => n.to_string(),
                    LlvmReq::AtLeast(n) => format!("{n}+"),
                })),
            );
        }
        if !self.libs.is_empty() || !self.pie {
            let libs = self.libs.iter().map(|lib| match lib {
                Lib::M => "m",
                Lib::Cxx => "c++",
            });
            let pie = (!self.pie).then_some("no-pie");
            line("link", &join(libs.chain(pie).map(str::to_owned)));
        }
        for (key, directives) in [("unsupported", &self.unsupported), ("xfail", &self.xfail)] {
            for directive in directives {
                let mut name = key.to_owned();
                if !directive.when.is_empty() {
                    let conds: Vec<String> = directive.when.iter().map(Cond::to_string).collect();
                    let _ = write!(name, "({})", conds.join(","));
                }
                line(&name, &directive.reason);
            }
        }
        if !self.args.is_empty() {
            line("args", &self.args.join(" "));
        }
        for text in String::from_utf8_lossy(&self.stdin).split_terminator('\n') {
            line("stdin", text);
        }
        if self.exit != Exit::default() {
            line("exit", &self.exit.to_string());
        }
        for text in String::from_utf8_lossy(&self.stdout).split_terminator('\n') {
            line("stdout", text);
        }
        out
    }
}

fn join(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(" ")
}

/// A directive line that could not be accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectiveError {
    /// The line, counted from 1.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for DirectiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// Reads the directives of an IR file: every line that starts with `;@`.
/// All errors are reported, in line order.
pub fn parse(text: &str) -> Result<Expectations, Vec<DirectiveError>> {
    let mut parser = Parser::default();
    for (index, line) in text.split('\n').enumerate() {
        if let Some(rest) = line.strip_prefix(";@")
            && let Err(message) = parser.directive(rest, index + 1)
        {
            parser.errors.push(DirectiveError { line: index + 1, message });
        }
    }
    if parser.errors.is_empty() { Ok(parser.found) } else { Err(parser.errors) }
}

const MALFORMED: &str = "malformed directive: expected ';@ <key>: <value>'";

#[derive(Default)]
struct Parser {
    found: Expectations,
    /// The keys that may appear once, with the line each was seen on.
    seen: Vec<(String, usize)>,
    errors: Vec<DirectiveError>,
}

impl Parser {
    /// `rest` is the line after `;@`.
    fn directive(&mut self, rest: &str, line: usize) -> Result<(), String> {
        let rest = rest.strip_prefix(' ').ok_or(MALFORMED)?;
        let key_len = rest.bytes().take_while(|b| b.is_ascii_lowercase() || *b == b'-').count();
        let (key, after) = rest.split_at(key_len);
        if key.is_empty() {
            return Err(MALFORMED.to_owned());
        }
        let (conds, after) = match after.strip_prefix('(') {
            Some(inner) => {
                let (list, tail) = inner
                    .split_once(')')
                    .ok_or_else(|| format!("malformed directive: missing ')' after '{key}('"))?;
                (Some(list), tail)
            }
            None => (None, after),
        };
        let value = after
            .strip_prefix(':')
            .ok_or_else(|| format!("malformed directive: expected ':' after '{key}'"))?;
        let value = value.strip_prefix(' ').unwrap_or(value);

        let conditional = matches!(key, "xfail" | "unsupported");
        let known = conditional
            || matches!(
                key,
                "stdout"
                    | "stdin"
                    | "exit"
                    | "args"
                    | "target"
                    | "llvm"
                    | "link"
                    | "tier"
                    | "covers"
            );
        if !known {
            return Err(format!("unknown directive '{key}'"));
        }
        if conds.is_some() && !conditional {
            return Err(format!("directive '{key}' takes no conditions"));
        }
        if !conditional && !matches!(key, "stdout" | "stdin") {
            if let Some((_, first)) = self.seen.iter().find(|(seen, _)| seen == key) {
                return Err(format!("duplicate directive '{key}' (first on line {first})"));
            }
            self.seen.push((key.to_owned(), line));
            if value.trim().is_empty() {
                return Err(format!("directive '{key}' requires a value"));
            }
        }
        let found = &mut self.found;
        match key {
            "stdout" => {
                found.stdout.extend_from_slice(value.as_bytes());
                found.stdout.push(b'\n');
            }
            "stdin" => {
                found.stdin.extend_from_slice(value.as_bytes());
                found.stdin.push(b'\n');
            }
            "exit" => found.exit = parse_exit(value)?,
            "args" => found.args = words(value).map(str::to_owned).collect(),
            "target" => found.targets = parse_targets(value)?,
            "llvm" => found.llvm = parse_llvm(value)?,
            "link" => (found.libs, found.pie) = parse_link(value)?,
            "tier" => {
                found.tier = match value.trim() {
                    "quick" => Tier::Quick,
                    "full" => Tier::Full,
                    other => {
                        return Err(format!("unknown tier '{other}': expected quick or full"));
                    }
                };
            }
            "covers" => found.covers = Some(value.trim().to_owned()),
            "xfail" => {
                let when = parse_conds(conds, true)?;
                let reason = value.trim();
                if reason.is_empty() {
                    return Err("'xfail' requires a reason".to_owned());
                }
                found.xfail.push(Conditional { when, reason: reason.to_owned() });
            }
            "unsupported" => {
                let when = parse_conds(conds, false)?;
                let reason = value.trim();
                let valid = !reason.is_empty()
                    && reason
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
                if !valid {
                    return Err(format!(
                        "'unsupported' requires a coverage-log reason \
                         (lower-case letters, digits and '-'), not '{reason}'"
                    ));
                }
                found.unsupported.push(Conditional { when, reason: reason.to_owned() });
            }
            _ => unreachable!("every known key is handled above"),
        }
        Ok(())
    }
}

/// The items of a list value: separated by spaces and tabs.
fn words(value: &str) -> impl Iterator<Item = &str> {
    value.split([' ', '\t']).filter(|word| !word.is_empty())
}

fn parse_exit(value: &str) -> Result<Exit, String> {
    let invalid =
        || format!("invalid exit status '{}': expected 0..255 or 'signal N'", value.trim());
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let items: Vec<&str> = words(value).collect();
    match items[..] {
        [code] if digits(code) => code.parse().map(Exit::Code).map_err(|_| invalid()),
        ["signal", signal] if digits(signal) => match signal.parse::<u8>() {
            Ok(n @ 1..=64) => Ok(Exit::Signal(n)),
            _ => Err(invalid()),
        },
        _ => Err(invalid()),
    }
}

fn parse_targets(value: &str) -> Result<Vec<Arch>, String> {
    let mut targets = Vec::new();
    for word in words(value) {
        let arch = Arch::from_name(word)
            .ok_or_else(|| format!("unknown target '{word}': expected aarch64 or x86_64"))?;
        if targets.contains(&arch) {
            return Err(format!("target '{word}' is listed twice"));
        }
        targets.push(arch);
    }
    Ok(targets)
}

fn parse_major(text: &str) -> Option<u32> {
    let plain =
        !text.is_empty() && !text.starts_with('0') && text.bytes().all(|b| b.is_ascii_digit());
    if plain { text.parse().ok() } else { None }
}

fn parse_llvm(value: &str) -> Result<Vec<LlvmReq>, String> {
    words(value)
        .map(|word| {
            let (digits, at_least) = match word.strip_suffix('+') {
                Some(digits) => (digits, true),
                None => (word, false),
            };
            let major = parse_major(digits).ok_or_else(|| {
                format!("invalid LLVM version '{word}': expected a major 'N' or 'N+'")
            })?;
            Ok(if at_least { LlvmReq::AtLeast(major) } else { LlvmReq::Exactly(major) })
        })
        .collect()
}

fn parse_link(value: &str) -> Result<(Vec<Lib>, bool), String> {
    let mut libs = Vec::new();
    let mut pie = None;
    for word in words(value) {
        let lib = match word {
            "m" => Lib::M,
            "c++" => Lib::Cxx,
            "pie" | "no-pie" => {
                let wanted = word == "pie";
                if pie.is_some_and(|earlier| earlier != wanted) {
                    return Err("'link' has both 'pie' and 'no-pie'".to_owned());
                }
                pie = Some(wanted);
                continue;
            }
            _ => {
                return Err(format!("unknown link item '{word}': expected m, c++, pie or no-pie"));
            }
        };
        if libs.contains(&lib) {
            return Err(format!("link item '{word}' is listed twice"));
        }
        libs.push(lib);
    }
    Ok((libs, pie.unwrap_or(true)))
}

/// `list` is the text between the parentheses, if there were any.
fn parse_conds(list: Option<&str>, linkers_allowed: bool) -> Result<Vec<Cond>, String> {
    let Some(list) = list else { return Ok(Vec::new()) };
    let mut conds: Vec<Cond> = Vec::new();
    for item in list.split(',') {
        let cond = if let Some(arch) = Arch::from_name(item) {
            Cond::Arch(arch)
        } else if LINKER_NAMES.contains(&item) {
            if !linkers_allowed {
                return Err(format!(
                    "'unsupported' cannot depend on a linker ('{item}'): hod declines before linking"
                ));
            }
            Cond::Linker(item.to_owned())
        } else if let Some(major) = item.strip_prefix("llvm").and_then(parse_major) {
            Cond::Llvm(major)
        } else {
            return Err(format!(
                "unknown condition '{item}': expected a target (aarch64, x86_64), \
                 a linker (bfd, lld, mold, wild) or llvm<N>"
            ));
        };
        let same_kind =
            |other: &Cond| std::mem::discriminant(other) == std::mem::discriminant(&cond);
        if conds.iter().any(same_kind) {
            return Err(format!(
                "condition '{item}' is a second condition of its kind; all conditions must hold"
            ));
        }
        conds.push(cond);
    }
    Ok(conds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Expectations {
        parse(text).unwrap_or_else(|errors| panic!("{text:?}: {errors:?}"))
    }

    /// The single error of a file with one bad line.
    fn error(text: &str) -> String {
        let errors = parse(text).expect_err(text);
        assert_eq!(errors.len(), 1, "{text:?}: {errors:?}");
        errors[0].to_string()
    }

    // spec/harness.md "Test directives": a file without directives is a
    // program that prints nothing and exits 0, valid everywhere.
    #[test]
    fn defaults() {
        let found = ok("define i32 @main() {\n  ret i32 0\n}\n");
        assert_eq!(found, Expectations::default());
        assert!(found.stdout.is_empty() && found.stdin.is_empty() && found.args.is_empty());
        assert_eq!(found.exit, Exit::Code(0));
        assert!(found.pie && found.libs.is_empty());
        assert_eq!(found.tier, Tier::Full);
        assert!(found.runs_on(Arch::Aarch64) && found.runs_on(Arch::X86_64));
        assert!(found.accepts_llvm(19) && found.accepts_llvm(22));
        assert_eq!(found.unsupported_reason(Arch::Aarch64, 22), None);
        assert_eq!(found.xfail_reason(Arch::Aarch64, 22, Some("bfd")), None);
    }

    // "Test directives", `stdout` and `stdin`: one line each, in order,
    // verbatim after one optional space.
    #[test]
    fn stdout_and_stdin_lines() {
        let found = ok(";@ stdout: first\n;@ stdout:\n;@ stdout:  two spaces \n;@ stdout:tight\n\
                        define void @f() {\n;@ stdout: later: with a colon ;@ and more\n\
                        ;@ stdin: in 1\n;@ stdin: in 2\n");
        assert_eq!(
            String::from_utf8(found.stdout).unwrap(),
            "first\n\n two spaces \ntight\nlater: with a colon ;@ and more\n"
        );
        assert_eq!(found.stdin, b"in 1\nin 2\n");
        // A carriage return is part of the text: lines end at a line feed.
        assert_eq!(ok(";@ stdout: a\r\n").stdout, b"a\r\n");
        // The last line needs no line feed.
        assert_eq!(ok(";@ stdout: end").stdout, b"end\n");
    }

    // "Test directives", `exit`.
    #[test]
    fn exit_status() {
        assert_eq!(ok(";@ exit: 0").exit, Exit::Code(0));
        assert_eq!(ok(";@ exit: 255").exit, Exit::Code(255));
        assert_eq!(ok(";@ exit:   7  ").exit, Exit::Code(7));
        assert_eq!(ok(";@ exit: signal 6").exit, Exit::Signal(6));
        assert_eq!(ok(";@ exit: signal\t64").exit, Exit::Signal(64));
        for bad in ["256", "-1", "+1", "0x10", "signal", "signal 0", "signal 65", "1 2", "abort"] {
            let message = error(&format!(";@ exit: {bad}"));
            assert!(message.contains("invalid exit status"), "{bad}: {message}");
        }
    }

    // "Test directives", `args`, `target`, `llvm`, `link`, `tier`, `covers`.
    #[test]
    fn list_and_word_directives() {
        let found = ok(";@ args: one  two\tthree\n;@ target: x86_64 aarch64\n;@ llvm: 19 21+\n\
                        ;@ link: c++ m no-pie\n;@ tier: quick\n;@ covers:  spec/ir.md §5.4 integer \n");
        assert_eq!(found.args, ["one", "two", "three"]);
        assert_eq!(found.targets, [Arch::X86_64, Arch::Aarch64]);
        assert_eq!(found.llvm, [LlvmReq::Exactly(19), LlvmReq::AtLeast(21)]);
        assert!(found.accepts_llvm(19) && !found.accepts_llvm(20));
        assert!(found.accepts_llvm(21) && found.accepts_llvm(22) && !found.accepts_llvm(18));
        assert_eq!(found.libs, [Lib::Cxx, Lib::M]);
        assert!(!found.pie);
        assert_eq!(found.tier, Tier::Quick);
        assert_eq!(found.covers.as_deref(), Some("spec/ir.md §5.4 integer"));

        let one = ok(";@ target: aarch64\n;@ link: pie\n;@ tier: full\n");
        assert!(one.runs_on(Arch::Aarch64) && !one.runs_on(Arch::X86_64));
        assert!(one.pie && one.libs.is_empty());
        assert_eq!(one.tier, Tier::Full);
        assert!(ok(";@ link: pie m pie").pie);
    }

    // "Test directives", `unsupported` and `xfail`, and their conditions.
    #[test]
    fn conditional_directives() {
        let found = ok(";@ unsupported(x86_64): codegen-not-implemented\n\
                        ;@ unsupported(aarch64,llvm19): bitcode-input\n\
                        ;@ xfail(mold): BACKLOG WP-099: mold rejects the note section\n\
                        ;@ xfail(x86_64,lld,llvm22): three at once\n");
        assert_eq!(
            found.unsupported,
            [
                Conditional {
                    when: vec![Cond::Arch(Arch::X86_64)],
                    reason: "codegen-not-implemented".to_owned()
                },
                Conditional {
                    when: vec![Cond::Arch(Arch::Aarch64), Cond::Llvm(19)],
                    reason: "bitcode-input".to_owned()
                },
            ]
        );
        assert_eq!(found.unsupported_reason(Arch::X86_64, 22), Some("codegen-not-implemented"));
        assert_eq!(found.unsupported_reason(Arch::Aarch64, 19), Some("bitcode-input"));
        assert_eq!(found.unsupported_reason(Arch::Aarch64, 22), None);

        let mold = Some("BACKLOG WP-099: mold rejects the note section");
        assert_eq!(found.xfail_reason(Arch::Aarch64, 22, Some("mold")), mold);
        assert_eq!(found.xfail_reason(Arch::Aarch64, 22, Some("bfd")), None);
        assert_eq!(found.xfail_reason(Arch::X86_64, 22, Some("lld")), Some("three at once"));
        assert_eq!(found.xfail_reason(Arch::X86_64, 19, Some("lld")), None);
        // Not linked: a condition on a linker does not hold.
        assert_eq!(found.xfail_reason(Arch::Aarch64, 22, None), None);

        // Without conditions a directive applies everywhere; the first
        // that applies wins.
        let both = ok(";@ xfail: always\n;@ xfail(bfd): never reached\n;@ unsupported: a-1\n");
        assert_eq!(both.xfail_reason(Arch::X86_64, 19, None), Some("always"));
        assert_eq!(both.xfail_reason(Arch::X86_64, 19, Some("bfd")), Some("always"));
        assert_eq!(both.unsupported, [Conditional::always("a-1")]);
    }

    // "Test directives", "Errors": every one of them.
    #[test]
    fn errors() {
        for (text, needle) in [
            (";@stdout: x", "malformed directive: expected ';@ <key>: <value>'"),
            (";@", "malformed directive"),
            (";@ ", "malformed directive"),
            (";@  stdout: x", "malformed directive"),
            (";@ Stdout: x", "malformed directive"),
            (";@x = global i32 0", "malformed directive"),
            (";@ stdout", "expected ':' after 'stdout'"),
            (";@ stdout : x", "expected ':' after 'stdout'"),
            (";@ xfail(mold: x", "missing ')' after 'xfail('"),
            (";@ xfail(mold) : x", "expected ':' after 'xfail'"),
            (";@ stdot: x", "unknown directive 'stdot'"),
            (";@ expect-fail: x", "unknown directive 'expect-fail'"),
            (";@ stdout(aarch64): x", "directive 'stdout' takes no conditions"),
            (";@ tier(aarch64): quick", "directive 'tier' takes no conditions"),
            (";@ exit: 1\n;@ exit: 1", "duplicate directive 'exit' (first on line 1)"),
            (";@ args: a\n\n;@ args: b", "line 3: duplicate directive 'args' (first on line 1)"),
            (";@ target:", "directive 'target' requires a value"),
            (";@ args: \t", "directive 'args' requires a value"),
            (";@ covers:", "directive 'covers' requires a value"),
            (";@ target: arm64", "unknown target 'arm64'"),
            (";@ target: aarch64 aarch64", "target 'aarch64' is listed twice"),
            (";@ llvm: 22.1", "invalid LLVM version '22.1'"),
            (";@ llvm: >=20", "invalid LLVM version '>=20'"),
            (";@ llvm: 019", "invalid LLVM version '019'"),
            (";@ llvm: +", "invalid LLVM version '+'"),
            (";@ link: stdc++", "unknown link item 'stdc++'"),
            (";@ link: m m", "link item 'm' is listed twice"),
            (";@ link: pie no-pie", "'link' has both 'pie' and 'no-pie'"),
            (";@ tier: slow", "unknown tier 'slow'"),
            (";@ xfail:", "'xfail' requires a reason"),
            (";@ xfail(mold):  ", "'xfail' requires a reason"),
            (";@ unsupported:", "'unsupported' requires a coverage-log reason"),
            (";@ unsupported: Codegen", "'unsupported' requires a coverage-log reason"),
            (";@ unsupported: two words", "'unsupported' requires a coverage-log reason"),
            (";@ unsupported(mold): x", "'unsupported' cannot depend on a linker ('mold')"),
            (";@ xfail(): x", "unknown condition ''"),
            (";@ xfail(gold): x", "unknown condition 'gold'"),
            (";@ xfail(aarch64, mold): x", "unknown condition ' mold'"),
            (";@ xfail(llvm): x", "unknown condition 'llvm'"),
            (";@ xfail(llvm22+): x", "unknown condition 'llvm22+'"),
            (";@ xfail(aarch64,x86_64): x", "condition 'x86_64' is a second condition of its kind"),
            (";@ xfail(bfd,llvm19,lld): x", "condition 'lld' is a second condition of its kind"),
        ] {
            let message = error(text);
            assert!(message.contains(needle), "{text:?}: {message}");
        }
        // Every bad line is reported, with its number; good lines between
        // them do not hide them.
        let errors = parse("; a comment\n;@ nope: 1\n;@ stdout: fine\n;@ exit: x\n").unwrap_err();
        let lines: Vec<usize> = errors.iter().map(|e| e.line).collect();
        assert_eq!(lines, [2, 4]);
        assert_eq!(errors[0].to_string(), "line 2: unknown directive 'nope'");
    }

    // "Test directives": only a line that starts with `;@` is a directive.
    #[test]
    fn only_lines_that_start_with_the_marker() {
        let found = ok("; @main is not a directive\n  ;@ stdout: indented, so a comment\n\
                        ; ;@ stdout: nor this\n@x = global i32 0 ;@ stdout: nor this\n");
        assert_eq!(found, Expectations::default());
    }

    /// The directive examples of `spec/harness.md`: the `llvm` code blocks
    /// of its "Test directives" section.
    fn spec_examples() -> Vec<String> {
        let spec = include_str!("../../../spec/harness.md");
        let section = spec.split("\n## Test directives\n").nth(1).expect("the section exists");
        let section = section.split("\n## ").next().unwrap();
        let blocks: Vec<String> = section
            .split("```llvm\n")
            .skip(1)
            .map(|rest| rest.split("```").next().unwrap().to_owned())
            .collect();
        assert!(blocks.len() >= 2, "the section has its examples");
        blocks
    }

    // "Test directives": the documented examples parse, mean what the
    // text says, and survive printing and parsing again.
    #[test]
    fn documented_examples_round_trip() {
        let examples = spec_examples();
        for example in &examples {
            let found = ok(example);
            let printed = found.to_directives();
            assert_eq!(ok(&printed), found, "{printed}");
            assert_eq!(ok(&printed).to_directives(), printed);
        }
        // The first example is the header of a test of the seed corpus.
        let first = ok(&examples[0]);
        assert_eq!(first, ok(include_str!("../../../tests/ir/main_args.ll")));
        assert_eq!(
            first.covers.as_deref(),
            Some("spec/ir.md §5.4 call, memory (the process interface every other test relies on)")
        );
        assert_eq!(first.tier, Tier::Quick);
        assert_eq!(first.args, ["alpha", "beta"]);
        assert_eq!(first.stdin, b"one line of input\n");
        assert_eq!(first.exit, Exit::Code(3));
        assert_eq!(first.stdout, b"argc=3\nalpha beta\n\none line of input\n");
        assert_eq!(first.unsupported, [Conditional::always("codegen-not-implemented")]);

        let second = ok(&examples[1]);
        assert_eq!(second.targets, [Arch::Aarch64]);
        assert_eq!(second.llvm, [LlvmReq::AtLeast(20)]);
        assert_eq!((second.libs.as_slice(), second.pie), (&[Lib::Cxx, Lib::M][..], false));
        assert_eq!(second.unsupported_reason(Arch::Aarch64, 22), None);
        assert_eq!(second.unsupported_reason(Arch::Aarch64, 21), Some("codegen-not-implemented"));
        assert_eq!(
            second.xfail_reason(Arch::Aarch64, 22, Some("mold")),
            Some("the reason says what fails and where it is tracked")
        );
        assert_eq!(second.xfail_reason(Arch::Aarch64, 22, Some("lld")), None);
        assert_eq!(second.exit, Exit::Signal(6));
    }

    // "Test directives", "Canonical form": every field is written, in the
    // documented order, and read back.
    #[test]
    fn canonical_form() {
        let all = Expectations {
            stdout: b"out 1\n\nout 3\n".to_vec(),
            stdin: b"in\n".to_vec(),
            exit: Exit::Signal(11),
            args: vec!["a".to_owned(), "b".to_owned()],
            targets: vec![Arch::Aarch64, Arch::X86_64],
            llvm: vec![LlvmReq::Exactly(19), LlvmReq::AtLeast(22)],
            libs: vec![Lib::M, Lib::Cxx],
            pie: false,
            tier: Tier::Quick,
            xfail: vec![
                Conditional {
                    when: vec![Cond::Linker("wild".to_owned()), Cond::Llvm(19)],
                    reason: "why: it fails".to_owned(),
                },
                Conditional::always("everywhere"),
            ],
            unsupported: vec![Conditional {
                when: vec![Cond::Arch(Arch::X86_64)],
                reason: "codegen-not-implemented".to_owned(),
            }],
            covers: Some("spec/ir.md §5.4".to_owned()),
        };
        let printed = all.to_directives();
        assert_eq!(
            printed,
            ";@ covers: spec/ir.md §5.4\n;@ tier: quick\n;@ target: aarch64 x86_64\n\
             ;@ llvm: 19 22+\n;@ link: m c++ no-pie\n\
             ;@ unsupported(x86_64): codegen-not-implemented\n\
             ;@ xfail(wild,llvm19): why: it fails\n;@ xfail: everywhere\n;@ args: a b\n\
             ;@ stdin: in\n;@ exit: signal 11\n;@ stdout: out 1\n;@ stdout:\n;@ stdout: out 3\n"
        );
        assert_eq!(ok(&printed), all);
        assert_eq!(Expectations::default().to_directives(), "");
    }
}
