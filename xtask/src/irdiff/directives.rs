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

/// An `oracle-skip` directive: the LLVM majors whose oracle is wrong for
/// the test, and why. There, the program of hod's object is compared with
/// the directives only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleSkip {
    /// The majors, at least one.
    pub llvm: Vec<u32>,
    pub reason: String,
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
    /// The link may warn (`link: allow-warnings`): it is done without
    /// `-Wl,--fatal-warnings`.
    pub allow_warnings: bool,
    /// Arguments both compilers get besides the harness's own
    /// (`llc-args`); each satisfies [`llc_arg_allowed`].
    pub llc_args: Vec<String>,
    /// The set the test belongs to.
    pub tier: Tier,
    /// Where the test is expected to fail, and why.
    pub xfail: Vec<Conditional>,
    /// Where hod is expected to decline the module, and with which reason.
    pub unsupported: Vec<Conditional>,
    /// The spec section the test covers.
    pub covers: Option<String>,
    /// The test is IR before optimisation (`input: pre-opt`): the oracle
    /// and the witness run `opt -passes='default<O0>'` before `llc`.
    pub pre_opt: bool,
    /// The LLVM major whose dialect the test is written in (`dialect`):
    /// it runs only with that LLVM's oracle.
    pub dialect: Option<u32>,
    /// Where the oracle is not used (`oracle-skip`).
    pub oracle_skip: Vec<OracleSkip>,
    /// CPU features the emulated runner's model gets for this test
    /// (`cpu`), without their `+`; the test is for one architecture.
    pub cpu: Vec<String>,
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
            allow_warnings: false,
            llc_args: Vec::new(),
            tier: Tier::default(),
            xfail: Vec::new(),
            unsupported: Vec::new(),
            covers: None,
            pre_opt: false,
            dialect: None,
            oracle_skip: Vec::new(),
            cpu: Vec::new(),
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

    /// Whether the test is in the dialect of this LLVM major: it has no
    /// `dialect` directive, or that one names `major`.
    pub fn in_dialect(&self, major: u32) -> bool {
        self.dialect.is_none_or(|dialect| dialect == major)
    }

    /// Why the oracle is not used with this LLVM major, if an
    /// `oracle-skip` directive names it: the first that does.
    pub fn oracle_skip_reason(&self, major: u32) -> Option<&str> {
        self.oracle_skip
            .iter()
            .find(|skip| skip.llvm.contains(&major))
            .map(|skip| skip.reason.as_str())
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
    /// `stdin` are UTF-8, `stdin` ends with a newline unless it is empty,
    /// no argument, reason or `covers` text is empty or has surrounding
    /// white space, and every `llc_args` item is an allowed one.
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
        if let Some(dialect) = self.dialect {
            line("dialect", &dialect.to_string());
        }
        if self.pre_opt {
            line("input", PRE_OPT);
        }
        if !self.libs.is_empty() || !self.pie || self.allow_warnings {
            let libs = self.libs.iter().map(|lib| match lib {
                Lib::M => "m",
                Lib::Cxx => "c++",
            });
            let pie = (!self.pie).then_some("no-pie");
            let warnings = self.allow_warnings.then_some("allow-warnings");
            line("link", &join(libs.chain(pie).chain(warnings).map(str::to_owned)));
        }
        if !self.llc_args.is_empty() {
            line("llc-args", &self.llc_args.join(" "));
        }
        if !self.cpu.is_empty() {
            line("cpu", &join(self.cpu.iter().map(|feature| format!("+{feature}"))));
        }
        for skip in &self.oracle_skip {
            let majors: Vec<String> = skip.llvm.iter().map(u32::to_string).collect();
            line(&format!("oracle-skip(llvm={})", majors.join(",")), &skip.reason);
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
        // The complete lines, then what follows the last newline.
        let stdout = String::from_utf8_lossy(&self.stdout);
        let (lines, rest) = match stdout.rsplit_once('\n') {
            Some((lines, rest)) => (Some(lines), rest),
            None => (None, &*stdout),
        };
        for text in lines.iter().flat_map(|lines| lines.split('\n')) {
            line("stdout", text);
        }
        if !rest.is_empty() {
            line("stdout-no-newline", rest);
        }
        out
    }
}

/// The value of `input` for IR before optimisation.
const PRE_OPT: &str = "pre-opt";

/// The `llc` options a test may pass with `llc-args`, by name: the ones a
/// compiler driver gives `llc` that change the code and name no file. A
/// name that ends in `=` takes a value.
const LLC_ARGS: [&str; 7] = [
    "-code-model=",
    "-data-sections",
    "-debugger-tune=",
    "-fp-contract=",
    "-function-sections",
    "-mcpu=",
    "-x86-branches-within-32B-boundaries",
];

/// The option of [`LLC_ARGS`] that `arg` is, if it is one: a flag exactly,
/// or `-name=<value>` with a value of letters, digits, `.`, `_`, `+`, `-`.
fn llc_option(arg: &str) -> Option<&'static str> {
    LLC_ARGS.into_iter().find(|option| match option.strip_suffix('=') {
        None => arg == *option,
        Some(_) => arg.strip_prefix(option).is_some_and(|value| {
            !value.is_empty()
                && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
        }),
    })
}

/// Whether a test may pass `arg` to both compilers with `llc-args`.
pub fn llc_arg_allowed(arg: &str) -> bool {
    llc_option(arg).is_some()
}

/// One module of a test file that holds several (`;@ module: <name>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub name: String,
    /// The module's text, with one empty line for each line of the file
    /// before it, so that a tool's diagnostics give the file's line numbers.
    pub text: String,
}

/// The modules of a test file: empty for a file without `module`
/// directives, which is one module as it stands. The file's directives
/// must parse ([`parse`] reports what is wrong with its `module` lines).
pub fn modules(text: &str) -> Vec<Module> {
    let mut found: Vec<Module> = Vec::new();
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if let Some(name) = line.strip_prefix(";@ module:") {
            found.push(Module { name: name.trim().to_owned(), text: "\n".repeat(index) });
        }
        if let Some(module) = found.last_mut() {
            module.text.push_str(line);
        }
    }
    found
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
        if let Some(rest) = line.strip_prefix(";@") {
            if let Err(message) = parser.directive(rest, index + 1) {
                parser.errors.push(DirectiveError { line: index + 1, message });
            }
        } else if parser.modules.is_empty() && !(line.trim().is_empty() || line.starts_with(';')) {
            parser.code_first = true;
        }
    }
    if let Some((_, line)) = parser.seen.iter().find(|(key, _)| key == "cpu")
        && parser.found.targets.len() != 1
    {
        parser.errors.push(DirectiveError {
            line: *line,
            message: "'cpu' names features of one architecture: the test needs a 'target' \
                      directive with exactly one"
                .to_owned(),
        });
    }
    if parser.errors.is_empty() { Ok(parser.found) } else { Err(parser.errors) }
}

const MALFORMED: &str = "malformed directive: expected ';@ <key>: <value>'";

#[derive(Default)]
struct Parser {
    found: Expectations,
    /// The keys that may appear once, with the line each was seen on.
    seen: Vec<(String, usize)>,
    /// The names of the `module` directives so far.
    modules: Vec<String>,
    /// Code was seen before any `module` directive.
    code_first: bool,
    errors: Vec<DirectiveError>,
}

impl Parser {
    /// `rest` is the line after `;@`.
    #[expect(clippy::too_many_lines, reason = "one arm per key of the directive table")]
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

        let conditional = matches!(key, "xfail" | "unsupported" | "oracle-skip");
        let known = conditional
            || matches!(
                key,
                "stdout"
                    | "stdin"
                    | "exit"
                    | "args"
                    | "target"
                    | "llvm"
                    | "dialect"
                    | "input"
                    | "link"
                    | "tier"
                    | "covers"
                    | "llc-args"
                    | "cpu"
                    | "stdout-no-newline"
                    | "module"
            );
        if !known {
            return Err(format!("unknown directive '{key}'"));
        }
        if conds.is_some() && !conditional {
            return Err(format!("directive '{key}' takes no conditions"));
        }
        if !conditional && !matches!(key, "stdout" | "stdin" | "module") {
            if let Some((_, first)) = self.seen.iter().find(|(seen, _)| seen == key) {
                return Err(format!("duplicate directive '{key}' (first on line {first})"));
            }
            self.seen.push((key.to_owned(), line));
            // The text of an unterminated line is verbatim, white space too.
            let empty = if key == "stdout-no-newline" { value } else { value.trim() }.is_empty();
            if empty {
                return Err(format!("directive '{key}' requires a value"));
            }
        }
        let unterminated =
            self.seen.iter().find(|(seen, _)| seen == "stdout-no-newline").map(|(_, line)| *line);
        let found = &mut self.found;
        match key {
            "stdout" => {
                if let Some(first) = unterminated {
                    return Err(format!(
                        "'stdout' after 'stdout-no-newline' (line {first}): \
                         the line without a newline is the last of the output"
                    ));
                }
                found.stdout.extend_from_slice(value.as_bytes());
                found.stdout.push(b'\n');
            }
            "stdout-no-newline" => found.stdout.extend_from_slice(value.as_bytes()),
            "llc-args" => found.llc_args = parse_llc_args(value)?,
            "cpu" => found.cpu = parse_cpu(value)?,
            "module" => {
                let name = value.trim();
                let valid = name.starts_with(|c: char| c.is_ascii_alphanumeric())
                    && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
                if !valid {
                    return Err(format!(
                        "invalid module name '{name}': expected letters, digits, '.', '_' \
                         and '-', starting with a letter or a digit"
                    ));
                }
                if self.modules.iter().any(|earlier| earlier == name) {
                    return Err(format!("module '{name}' is named twice"));
                }
                self.modules.push(name.to_owned());
                if self.modules.len() == 1 && self.code_first {
                    return Err(format!(
                        "module '{name}' starts after code: only directives, comments and \
                         empty lines may come before the first 'module' directive"
                    ));
                }
            }
            "stdin" => {
                found.stdin.extend_from_slice(value.as_bytes());
                found.stdin.push(b'\n');
            }
            "exit" => found.exit = parse_exit(value)?,
            "args" => found.args = words(value).map(str::to_owned).collect(),
            "target" => found.targets = parse_targets(value)?,
            "llvm" => {
                if found.dialect.is_some() {
                    return Err("'llvm' and 'dialect' exclude each other".to_owned());
                }
                found.llvm = parse_llvm(value)?;
            }
            "dialect" => {
                if !found.llvm.is_empty() {
                    return Err("'llvm' and 'dialect' exclude each other".to_owned());
                }
                let major = parse_major(value.trim()).ok_or_else(|| {
                    format!("invalid dialect '{}': expected an LLVM major", value.trim())
                })?;
                found.dialect = Some(major);
            }
            "input" => {
                if value.trim() != PRE_OPT {
                    return Err(format!("unknown input '{}': expected {PRE_OPT}", value.trim()));
                }
                found.pre_opt = true;
            }
            "oracle-skip" => {
                let llvm = parse_skip_majors(conds)?;
                let reason = value.trim();
                if reason.is_empty() {
                    return Err("'oracle-skip' requires a reason".to_owned());
                }
                found.oracle_skip.push(OracleSkip { llvm, reason: reason.to_owned() });
            }
            "link" => (found.libs, found.pie, found.allow_warnings) = parse_link(value)?,
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
                let valid = reason == crate::irdiff::engine::ANY_REASON
                    || !reason.is_empty()
                        && reason.bytes().all(|b| {
                            b.is_ascii_lowercase() || b.is_ascii_digit() || b"-:._".contains(&b)
                        });
                if !valid {
                    return Err(format!(
                        "'unsupported' requires a coverage-log reason \
                         (lower-case letters, digits, '-', ':', '.' and '_') or '*', not '{reason}'"
                    ));
                }
                found.unsupported.push(Conditional { when, reason: reason.to_owned() });
            }
            _ => unreachable!("every known key is handled above"),
        }
        Ok(())
    }
}

/// The features of `cpu`: words `+<feature>`, a feature of lower-case
/// letters, digits, `.`, `_` and `-` (QEMU's names: `cx16`, `popcnt`,
/// `sse4.2`, `lahf-lm`), each once.
fn parse_cpu(value: &str) -> Result<Vec<String>, String> {
    let mut features: Vec<String> = Vec::new();
    for word in words(value) {
        let feature = word.strip_prefix('+').unwrap_or("");
        let valid = !feature.is_empty()
            && feature
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b));
        if !valid {
            return Err(format!(
                "invalid cpu feature '{word}': expected '+<feature>' (lower-case letters, \
                 digits, '.', '_' and '-')"
            ));
        }
        if features.iter().any(|seen| seen == feature) {
            return Err(format!("cpu feature '{word}' is given twice"));
        }
        features.push(feature.to_owned());
    }
    Ok(features)
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

fn parse_llc_args(value: &str) -> Result<Vec<String>, String> {
    /// The options the harness sets itself, for each compiler.
    const OWN: [&str; 6] =
        ["-o", "-filetype", "-mtriple", "-relocation-model", "-global-isel", "-fast-isel"];
    let mut args: Vec<String> = Vec::new();
    let mut options = Vec::new();
    for word in words(value) {
        let Some(option) = llc_option(word) else {
            let name = word.split('=').next().unwrap_or(word);
            return Err(if word.starts_with("-O") || OWN.contains(&name) {
                format!("llc argument '{word}' is the harness's to give")
            } else {
                format!(
                    "llc argument '{word}' is not one a test may give: expected {}",
                    LLC_ARGS.map(|option| option.replace('=', "=<value>")).join(", ")
                )
            });
        };
        if options.contains(&option) {
            return Err(format!("llc argument '{}' is given twice", option.trim_end_matches('=')));
        }
        options.push(option);
        args.push(word.to_owned());
    }
    Ok(args)
}

fn parse_link(value: &str) -> Result<(Vec<Lib>, bool, bool), String> {
    let mut libs = Vec::new();
    let mut pie = None;
    let mut allow_warnings = false;
    for word in words(value) {
        let lib = match word {
            "m" => Lib::M,
            "c++" => Lib::Cxx,
            "allow-warnings" => {
                if allow_warnings {
                    return Err(format!("link item '{word}' is listed twice"));
                }
                allow_warnings = true;
                continue;
            }
            "pie" | "no-pie" => {
                let wanted = word == "pie";
                if pie.is_some_and(|earlier| earlier != wanted) {
                    return Err("'link' has both 'pie' and 'no-pie'".to_owned());
                }
                pie = Some(wanted);
                continue;
            }
            _ => {
                return Err(format!(
                    "unknown link item '{word}': expected m, c++, pie, no-pie or allow-warnings"
                ));
            }
        };
        if libs.contains(&lib) {
            return Err(format!("link item '{word}' is listed twice"));
        }
        libs.push(lib);
    }
    Ok((libs, pie.unwrap_or(true), allow_warnings))
}

/// The majors of `oracle-skip(llvm=N,M)`: `list` is the text between the
/// parentheses.
fn parse_skip_majors(list: Option<&str>) -> Result<Vec<u32>, String> {
    let expected = "'oracle-skip' requires its LLVM majors: 'oracle-skip(llvm=N[,M...]): <reason>'";
    let majors = list.and_then(|list| list.strip_prefix("llvm=")).ok_or(expected)?;
    let mut found: Vec<u32> = Vec::new();
    for item in majors.split(',') {
        let major = parse_major(item)
            .ok_or_else(|| format!("invalid LLVM major '{item}' in 'oracle-skip'"))?;
        if found.contains(&major) {
            return Err(format!("LLVM {major} is listed twice in 'oracle-skip'"));
        }
        found.push(major);
    }
    Ok(found)
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

    // "Test directives", `stdout-no-newline`: the end of an output that
    // does not end in a newline, verbatim; once, and last.
    #[test]
    fn output_without_a_final_newline() {
        assert_eq!(ok(";@ stdout: a\n;@ stdout-no-newline: b").stdout, b"a\nb");
        assert_eq!(ok(";@ stdout-no-newline:  two  ").stdout, b" two  ");
        assert_eq!(ok(";@ stdout-no-newline:  ").stdout, b" ");
        assert_eq!(ok(";@ stdout-no-newline:tight\n;@ exit: 1\n").stdout, b"tight");
        for (text, printed) in [
            (&b"a\nb"[..], ";@ stdout: a\n;@ stdout-no-newline: b\n"),
            (b"only", ";@ stdout-no-newline: only\n"),
            (b"\n\n x", ";@ stdout:\n;@ stdout:\n;@ stdout-no-newline:  x\n"),
            (b"a\n", ";@ stdout: a\n"),
        ] {
            let expect = Expectations { stdout: text.to_vec(), ..Expectations::default() };
            assert_eq!(expect.to_directives(), printed);
            assert_eq!(ok(printed), expect);
        }
        for (text, needle) in [
            (";@ stdout-no-newline:", "directive 'stdout-no-newline' requires a value"),
            (
                ";@ stdout-no-newline: a\n;@ stdout-no-newline: b",
                "line 2: duplicate directive 'stdout-no-newline' (first on line 1)",
            ),
            (
                ";@ stdout-no-newline: a\n;@ stdout: b",
                "line 2: 'stdout' after 'stdout-no-newline' (line 1)",
            ),
            (";@ stdout-no-newline(mold): a", "directive 'stdout-no-newline' takes no conditions"),
        ] {
            let message = error(text);
            assert!(message.contains(needle), "{text:?}: {message}");
        }
    }

    // "Test directives", `llc-args`: an allow-list, each option once, in
    // the order written.
    #[test]
    fn llc_arguments() {
        assert!(ok("").llc_args.is_empty());
        let found = ok(";@ llc-args:  -debugger-tune=gdb\t-function-sections  -mcpu=cortex-a76 ");
        assert_eq!(
            found.llc_args,
            ["-debugger-tune=gdb", "-function-sections", "-mcpu=cortex-a76"]
        );
        let printed = found.to_directives();
        assert_eq!(
            printed,
            ";@ llc-args: -debugger-tune=gdb -function-sections -mcpu=cortex-a76\n"
        );
        assert_eq!(ok(&printed), found);
        for allowed in [
            "-code-model=small",
            "-data-sections",
            "-debugger-tune=lldb",
            "-fp-contract=fast",
            "-function-sections",
            "-mcpu=apple-m1",
            "-x86-branches-within-32B-boundaries",
        ] {
            assert!(llc_arg_allowed(allowed), "{allowed}");
            assert_eq!(ok(&format!(";@ llc-args: {allowed}")).llc_args, [allowed]);
        }
        for (bad, needle) in [
            ("-O2", "llc argument '-O2' is the harness's to give"),
            ("-O0", "is the harness's to give"),
            ("-o x.o", "llc argument '-o' is the harness's to give"),
            ("-filetype=asm", "llc argument '-filetype=asm' is the harness's to give"),
            ("-mtriple=x86_64-unknown-linux-gnu", "is the harness's to give"),
            ("-relocation-model=static", "is the harness's to give"),
            ("-global-isel=1", "llc argument '-global-isel=1' is the harness's to give"),
            ("-fast-isel", "llc argument '-fast-isel' is the harness's to give"),
            ("-split-dwarf-file=a.dwo", "is not one a test may give: expected -code-model=<value>"),
            ("-debugger-tune", "llc argument '-debugger-tune' is not one a test may give"),
            ("-debugger-tune=", "is not one a test may give"),
            ("-mcpu=a/b", "is not one a test may give"),
            ("-function-sections=1", "is not one a test may give"),
            ("--function-sections", "is not one a test may give"),
            ("input.ll", "llc argument 'input.ll' is not one a test may give"),
            ("-mcpu=a -mcpu=b", "llc argument '-mcpu' is given twice"),
            ("-data-sections -data-sections", "llc argument '-data-sections' is given twice"),
        ] {
            assert!(!llc_arg_allowed(bad), "{bad}");
            let message = error(&format!(";@ llc-args: {bad}"));
            assert!(message.contains(needle), "{bad}: {message}");
        }
        assert!(error(";@ llc-args:").contains("directive 'llc-args' requires a value"));
        assert!(
            error(";@ llc-args: -data-sections\n;@ llc-args: -function-sections")
                .contains("duplicate directive 'llc-args'")
        );
    }

    // "Test directives", "Several modules": `module` lines split the file.
    #[test]
    fn several_modules() {
        assert_eq!(modules("define i32 @main() {\n  ret i32 0\n}\n"), []);
        let text = "; a comment\n;@ stdout: x\n\n;@ module: lib\n@g = global i32 1\n\
                    ;@ exit: 1\n;@ module:  main.cpp \ndeclare void @f()\nlast";
        let found = ok(text);
        assert_eq!((found.stdout.as_slice(), found.exit), (&b"x\n"[..], Exit::Code(1)));
        let split = modules(text);
        assert_eq!(split.len(), 2);
        assert_eq!(split[0].name, "lib");
        assert_eq!(split[0].text, "\n\n\n;@ module: lib\n@g = global i32 1\n;@ exit: 1\n");
        assert_eq!(split[1].name, "main.cpp");
        assert_eq!(split[1].text, "\n\n\n\n\n\n;@ module:  main.cpp \ndeclare void @f()\nlast");
        // The directives are not part of the canonical header: they are
        // where the modules are.
        assert_eq!(found.to_directives(), ";@ exit: 1\n;@ stdout: x\n");
        // One module is a file of one module.
        assert_eq!(modules(";@ module: only\n@g = global i32 1\n").len(), 1);

        for (text, needle) in [
            (";@ module:", "invalid module name ''"),
            (";@ module: a b", "invalid module name 'a b'"),
            (";@ module: .hidden", "invalid module name '.hidden'"),
            (";@ module: dir/a", "invalid module name 'dir/a'"),
            (";@ module(aarch64): a", "directive 'module' takes no conditions"),
            (";@ module: a\n;@ module: b\n;@ module: a", "line 3: module 'a' is named twice"),
            (
                "; fine\n\n@g = global i32 1\n;@ module: a\n;@ module: b",
                "line 4: module 'a' starts after code",
            ),
        ] {
            let message = error(text);
            assert!(message.contains(needle), "{text:?}: {message}");
        }
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
        assert!(!one.allow_warnings && !found.allow_warnings);
        let lax = ok(";@ link: allow-warnings");
        assert!(lax.allow_warnings && lax.pie && lax.libs.is_empty());
        assert_eq!(lax.to_directives(), ";@ link: allow-warnings\n");
        let all = ok(";@ link: allow-warnings no-pie m");
        assert_eq!(all.to_directives(), ";@ link: m no-pie allow-warnings\n");
    }

    // "Test directives", `unsupported` and `xfail`, and their conditions.
    #[test]
    fn conditional_directives() {
        let any = ok(";@ unsupported: *\n");
        assert_eq!(any.unsupported_reason(Arch::X86_64, 19), Some("*"));
        assert_eq!(any.to_directives(), ";@ unsupported: *\n");
        assert!(error(";@ unsupported: **").contains("'unsupported' requires"));
        let names = ok(
            ";@ unsupported: intrinsic:llvm.memcpy.inline\n;@ unsupported: attr:returns_twice\n",
        );
        assert_eq!(
            names.unsupported_reason(Arch::Aarch64, 22),
            Some("intrinsic:llvm.memcpy.inline")
        );
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
            (
                ";@ link: allow-warnings m allow-warnings",
                "link item 'allow-warnings' is listed twice",
            ),
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
        let x86_64 = Conditional { when: vec![Cond::Arch(Arch::X86_64)], reason: "*".into() };
        assert_eq!(first.unsupported, [x86_64]);
        assert_eq!(first.unsupported_reason(Arch::Aarch64, 22), None);

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
            stdout: b"out 1\n\nout 3\nend".to_vec(),
            stdin: b"in\n".to_vec(),
            exit: Exit::Signal(11),
            args: vec!["a".to_owned(), "b".to_owned()],
            targets: vec![Arch::Aarch64, Arch::X86_64],
            llvm: vec![LlvmReq::Exactly(19), LlvmReq::AtLeast(22)],
            libs: vec![Lib::M, Lib::Cxx],
            pie: false,
            allow_warnings: true,
            llc_args: vec!["-function-sections".to_owned(), "-debugger-tune=gdb".to_owned()],
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
            pre_opt: true,
            dialect: None,
            oracle_skip: vec![
                OracleSkip { llvm: vec![19, 22], reason: "the oracle crashes".to_owned() },
                OracleSkip { llvm: vec![22], reason: "and another".to_owned() },
            ],
            cpu: Vec::new(),
        };
        let printed = all.to_directives();
        assert_eq!(
            printed,
            ";@ covers: spec/ir.md §5.4\n;@ tier: quick\n;@ target: aarch64 x86_64\n\
             ;@ llvm: 19 22+\n;@ input: pre-opt\n;@ link: m c++ no-pie allow-warnings\n\
             ;@ llc-args: -function-sections -debugger-tune=gdb\n\
             ;@ oracle-skip(llvm=19,22): the oracle crashes\n;@ oracle-skip(llvm=22): and another\n\
             ;@ unsupported(x86_64): codegen-not-implemented\n\
             ;@ xfail(wild,llvm19): why: it fails\n;@ xfail: everywhere\n;@ args: a b\n\
             ;@ stdin: in\n;@ exit: signal 11\n;@ stdout: out 1\n;@ stdout:\n;@ stdout: out 3\n\
             ;@ stdout-no-newline: end\n"
        );
        assert_eq!(ok(&printed), all);
        assert_eq!(Expectations::default().to_directives(), "");
        let dialect = Expectations { dialect: Some(22), ..Expectations::default() };
        assert_eq!(dialect.to_directives(), ";@ dialect: 22\n");
        assert_eq!(ok(";@ dialect: 22\n"), dialect);
    }

    // "Test directives", `cpu` (WP-139): CPU features of the emulated
    // runner, for a test of one architecture, written after `llc-args`.
    #[test]
    fn cpu_features() {
        let found = ok(";@ cpu: +cx16 +sse4.2\n;@ target: x86_64\n");
        assert_eq!(found.cpu, ["cx16", "sse4.2"]);
        assert_eq!(found.to_directives(), ";@ target: x86_64\n;@ cpu: +cx16 +sse4.2\n");
        let with_args = Expectations { llc_args: vec!["-mcpu=x86-64".to_owned()], ..found.clone() };
        assert_eq!(
            with_args.to_directives(),
            ";@ target: x86_64\n;@ llc-args: -mcpu=x86-64\n;@ cpu: +cx16 +sse4.2\n"
        );
        for (text, message) in [
            (";@ target: x86_64\n;@ cpu: cx16", "invalid cpu feature 'cx16'"),
            (";@ target: x86_64\n;@ cpu: -cx16", "invalid cpu feature '-cx16'"),
            (";@ target: x86_64\n;@ cpu: +CX16", "invalid cpu feature '+CX16'"),
            (";@ target: x86_64\n;@ cpu: +", "invalid cpu feature '+'"),
            (";@ target: x86_64\n;@ cpu: +cx16 +cx16", "is given twice"),
            (";@ target: x86_64\n;@ cpu:", "requires a value"),
            (";@ target: x86_64\n;@ cpu: +a\n;@ cpu: +b", "duplicate directive 'cpu'"),
            (";@ cpu: +cx16", "exactly one"),
            (";@ cpu: +cx16\n;@ target: x86_64 aarch64", "exactly one"),
            (";@ target: x86_64\n;@ cpu(x86_64): +cx16", "takes no conditions"),
        ] {
            assert!(error(text).contains(message), "{text}: {}", error(text));
        }
    }

    // "Test directives", `input`, `dialect` and `oracle-skip` (WP-105).
    #[test]
    fn pre_opt_dialects_and_oracle_skips() {
        let found = ok(";@ input: pre-opt\n;@ dialect: 19\n\
                        ;@ oracle-skip(llvm=19): crashes under poisoning\n\
                        ;@ oracle-skip(llvm=22,23): a second reason\n");
        assert!(found.pre_opt);
        assert!(found.in_dialect(19) && !found.in_dialect(22));
        assert_eq!(found.oracle_skip_reason(19), Some("crashes under poisoning"));
        assert_eq!(found.oracle_skip_reason(23), Some("a second reason"));
        assert_eq!(found.oracle_skip_reason(20), None);
        let plain = Expectations::default();
        assert!(!plain.pre_opt && plain.in_dialect(19) && plain.in_dialect(22));
        assert_eq!(plain.oracle_skip_reason(22), None);
        for (text, message) in [
            (";@ input: post-opt", "unknown input 'post-opt': expected pre-opt"),
            (";@ input: pre-opt\n;@ input: pre-opt", "duplicate directive 'input'"),
            (";@ dialect: llvm22", "invalid dialect 'llvm22'"),
            (";@ dialect: 22\n;@ llvm: 22", "'llvm' and 'dialect' exclude each other"),
            (";@ llvm: 19+\n;@ dialect: 19", "'llvm' and 'dialect' exclude each other"),
            (";@ dialect(aarch64): 22", "takes no conditions"),
            (";@ oracle-skip: why", "requires its LLVM majors"),
            (";@ oracle-skip(llvm19): why", "requires its LLVM majors"),
            (";@ oracle-skip(llvm=): why", "invalid LLVM major ''"),
            (";@ oracle-skip(llvm=19,x): why", "invalid LLVM major 'x'"),
            (";@ oracle-skip(llvm=19,19): why", "LLVM 19 is listed twice"),
            (";@ oracle-skip(llvm=22):  ", "'oracle-skip' requires a reason"),
        ] {
            let got = error(text);
            assert!(got.contains(message), "{text:?}: {got}");
        }
    }
}
