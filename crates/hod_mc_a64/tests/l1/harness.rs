//! The L1 differential harness (`spec/mc-a64.md` "Test method").
//!
//! A [`Batch`] collects `(encoded word, assembly text)` pairs. Checking it
//! assembles every line with one `llvm-mc` run and compares the words;
//! disassembles hod's own words with `llvm-objdump` and compares the
//! mnemonics; and feeds the operands hod's checked forms rejected to
//! `llvm-mc`, which must reject each of them too.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use hod_testutil::{Scratch, run, tool};

use crate::table::base_mnemonics;

/// One encoding to check.
#[derive(Debug)]
pub struct Case {
    /// The constructor that produced the word (without any `try_` prefix).
    pub name: &'static str,
    pub word: u32,
    /// The assembly line LLVM must encode to the same word.
    pub asm: String,
    /// The mnemonic LLVM must print for the word with aliases off, when the
    /// constructor has more than one (it takes an operation-kind operand).
    pub mnemonic: Option<String>,
}

/// An operand combination a checked form refused.
#[derive(Debug)]
struct Rejection {
    name: &'static str,
    /// The assembly LLVM must refuse too, when the combination has a text
    /// form that LLVM does not silently re-express as another instruction.
    asm: Option<String>,
}

/// A constructor result: a word, or the checked form's `Option`.
pub trait IntoWord {
    fn into_word(self, asm: &str) -> u32;
}

impl IntoWord for u32 {
    fn into_word(self, _: &str) -> u32 {
        self
    }
}

impl IntoWord for Option<u32> {
    fn into_word(self, asm: &str) -> u32 {
        self.unwrap_or_else(|| panic!("the checked form rejected valid operands: {asm}"))
    }
}

fn base_name(name: &'static str) -> &'static str {
    name.strip_prefix("try_").unwrap_or(name)
}

#[derive(Debug, Default)]
pub struct Batch {
    cases: Vec<Case>,
    rejections: Vec<Rejection>,
}

/// `(encodings, rejections)` per constructor.
pub type Counts = BTreeMap<&'static str, (usize, usize)>;

impl Batch {
    pub fn push(
        &mut self,
        name: &'static str,
        word: impl IntoWord,
        mnemonic: Option<String>,
        asm: String,
    ) {
        let word = word.into_word(&asm);
        self.cases.push(Case { name: base_name(name), word, asm, mnemonic });
    }

    /// Records that a checked form returned `None`, and fails at once if it
    /// did not.
    pub fn reject(
        &mut self,
        name: &'static str,
        result: Option<u32>,
        operands: &str,
        asm: Option<String>,
    ) {
        assert!(
            result.is_none(),
            "{name}{operands} must be rejected, but encoded {:#010x}",
            result.unwrap_or(0)
        );
        self.rejections.push(Rejection { name: base_name(name), asm });
    }

    /// Records a rejection the caller asserts, without calling a checked
    /// form: the harness's own tests use it.
    pub fn reject_text(&mut self, name: &'static str, asm: &str) {
        self.rejections.push(Rejection { name, asm: Some(asm.to_string()) });
    }

    /// Runs the three checks: the number of disagreements with LLVM and
    /// the report naming them.
    pub fn examine(&self, tools: &Tools) -> (usize, String) {
        let mut report = String::new();
        let mismatches = tools.compare(&self.cases, &mut report)
            + tools.reverse(&self.cases, &mut report)
            + tools.rejected(&self.rejections, &mut report);
        (mismatches, report)
    }

    pub fn counts(&self) -> Counts {
        let mut counts = Counts::new();
        for case in &self.cases {
            counts.entry(case.name).or_default().0 += 1;
        }
        for rejection in &self.rejections {
            counts.entry(rejection.name).or_default().1 += 1;
        }
        counts
    }

    /// Runs the differential checks and panics with a report of every
    /// disagreement. Skips (after a note) when LLVM's tools are missing and
    /// not required.
    pub fn check(self, label: &str) {
        let Some(tools) = Tools::find() else { return };
        let (mismatches, report) = self.examine(&tools);
        assert!(mismatches == 0, "{label}: {mismatches} disagreement(s) with LLVM\n{report}");
        let with_asm = self.rejections.iter().filter(|r| r.asm.is_some()).count();
        println!(
            "l1 {label}: {} encodings equal llvm-mc's and disassemble as intended; \
             {} rejections, {with_asm} of them confirmed by llvm-mc",
            self.cases.len(),
            self.rejections.len(),
        );
    }
}

/// At most this many disagreements are listed per check.
const REPORT_LIMIT: usize = 40;

#[derive(Debug)]
pub struct Tools {
    mc: PathBuf,
    objcopy: PathBuf,
    objdump: PathBuf,
    scratch: Scratch,
}

/// The features the oracle assembles and disassembles with. Without
/// `+fullfp16`, half-precision arithmetic is an error to it, as it is
/// absent from the crate.
const FEATURES: &str = "+lse,+bti,+pauth,+fp-armv8,+neon";

const TRIPLE: &str = "--triple=aarch64-unknown-linux-gnu";

impl Tools {
    pub fn find() -> Option<Self> {
        Some(Self {
            mc: tool("llvm-mc")?,
            objcopy: tool("llvm-objcopy")?,
            objdump: tool("llvm-objdump")?,
            scratch: Scratch::new("mc-a64"),
        })
    }

    /// Assembles `source` to an object file. `Err` holds `llvm-mc`'s
    /// diagnostics.
    fn assemble_object(&self, stem: &str, source: &str) -> Result<PathBuf, String> {
        let src = self.scratch.write(&format!("{stem}.s"), source);
        let obj = self.scratch.join(format!("{stem}.o"));
        let outcome = run(
            &self.mc,
            [
                TRIPLE.as_ref(),
                format!("-mattr={FEATURES}").as_ref(),
                "-filetype=obj".as_ref(),
                "-o".as_ref(),
                obj.as_os_str(),
                src.as_os_str(),
            ],
        );
        assert!(!outcome.timed_out(), "llvm-mc timed out on {stem}.s: an oracle ERROR");
        if outcome.success() { Ok(obj) } else { Err(outcome.stderr_text()) }
    }

    /// Assembles `source` and returns the words of its `.text`.
    pub fn assemble(&self, stem: &str, source: &str) -> Result<Vec<u32>, String> {
        let obj = self.assemble_object(stem, source)?;
        let bin = self.scratch.join(format!("{stem}.bin"));
        let outcome = run(
            &self.objcopy,
            [
                "-O".as_ref(),
                "binary".as_ref(),
                "--only-section=.text".as_ref(),
                obj.as_os_str(),
                bin.as_os_str(),
            ],
        );
        assert!(outcome.success(), "llvm-objcopy failed: {}", outcome.stderr_text());
        let bytes = std::fs::read(&bin).expect("llvm-objcopy wrote its output");
        assert!(bytes.len().is_multiple_of(4), ".text is not a whole number of words");
        Ok(bytes.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
    }

    /// The mnemonic LLVM's disassembler prints for each word, with aliases
    /// off.
    pub fn mnemonics(&self, stem: &str, words: &[u32]) -> Vec<String> {
        let mut source = String::new();
        for word in words {
            writeln!(source, ".inst {word:#010x}").unwrap();
        }
        let obj = self
            .assemble_object(stem, &source)
            .unwrap_or_else(|e| panic!("llvm-mc cannot assemble .inst words: {e}"));
        let outcome = run(
            &self.objdump,
            [
                "-d".as_ref(),
                "--no-show-raw-insn".as_ref(),
                "-M".as_ref(),
                "no-aliases".as_ref(),
                format!("--mattr={FEATURES}").as_ref(),
                obj.as_os_str(),
            ],
        );
        assert!(outcome.success(), "llvm-objdump failed: {}", outcome.stderr_text());
        let text = outcome.stdout_text();
        let mnemonics: Vec<String> = text
            .lines()
            .filter_map(|line| {
                let (address, rest) = line.split_once(':')?;
                let address = address.trim();
                let is_address =
                    !address.is_empty() && address.bytes().all(|b| b.is_ascii_hexdigit());
                is_address.then(|| rest.split_whitespace().next().unwrap_or("").to_string())
            })
            .collect();
        assert_eq!(mnemonics.len(), words.len(), "llvm-objdump printed one line per word");
        mnemonics
    }

    /// The 1-based source lines `llvm-mc` reported an error on, with the
    /// first message for each.
    fn error_lines(stderr: &str) -> BTreeMap<usize, String> {
        let mut lines = BTreeMap::new();
        for line in stderr.lines() {
            let Some((location, message)) = line.split_once(": error: ") else { continue };
            let mut parts = location.rsplitn(3, ':');
            let (_column, Some(number)) = (parts.next(), parts.next()) else { continue };
            if let Ok(number) = number.parse() {
                lines.entry(number).or_insert_with(|| message.to_string());
            }
        }
        lines
    }

    /// Forward direction: every assembly line encodes to hod's word.
    fn compare(&self, cases: &[Case], report: &mut String) -> usize {
        let mut source = String::new();
        for case in cases {
            assert!(!case.asm.contains('\n'), "one instruction per case");
            source.push_str(&case.asm);
            source.push('\n');
        }
        let words = match self.assemble("forward", &source) {
            Ok(words) => words,
            Err(stderr) => {
                let errors = Self::error_lines(&stderr);
                for (line, message) in errors.iter().take(REPORT_LIMIT) {
                    let case = &cases[line - 1];
                    writeln!(
                        report,
                        "  {}: llvm-mc refuses `{}` (hod: {:#010x}): {message}",
                        case.name, case.asm, case.word
                    )
                    .unwrap();
                }
                if errors.is_empty() {
                    writeln!(report, "  llvm-mc failed: {stderr}").unwrap();
                }
                return errors.len().max(1);
            }
        };
        assert_eq!(words.len(), cases.len(), "llvm-mc assembled one word per line");
        let mut mismatches = 0;
        for (case, llvm) in cases.iter().zip(&words) {
            if case.word != *llvm {
                mismatches += 1;
                if mismatches <= REPORT_LIMIT {
                    writeln!(
                        report,
                        "  {}: `{}`: hod {:#010x}, llvm-mc {llvm:#010x}",
                        case.name, case.asm, case.word
                    )
                    .unwrap();
                }
            }
        }
        mismatches
    }

    /// Reverse direction: hod's word disassembles to the instruction the
    /// constructor is specified to produce.
    fn reverse(&self, cases: &[Case], report: &mut String) -> usize {
        let words: Vec<u32> = cases.iter().map(|case| case.word).collect();
        let mnemonics = self.mnemonics("reverse", &words);
        let mut mismatches = 0;
        for (case, printed) in cases.iter().zip(&mnemonics) {
            if let Err(expected) = mnemonic_ok(case, printed) {
                mismatches += 1;
                if mismatches <= REPORT_LIMIT {
                    writeln!(
                        report,
                        "  {}: {:#010x} (`{}`) disassembles as `{printed}`, expected {expected}",
                        case.name, case.word, case.asm
                    )
                    .unwrap();
                }
            }
        }
        mismatches
    }

    /// Every rejection with assembly text is an error to LLVM as well.
    ///
    /// `llvm-mc` reports operand errors while parsing and displacement
    /// errors only if parsing succeeded, so lines without an error are
    /// retried until a run reports nothing new.
    fn rejected(&self, rejections: &[Rejection], report: &mut String) -> usize {
        let mut pending: Vec<(&'static str, &str)> =
            rejections.iter().filter_map(|r| Some((r.name, r.asm.as_deref()?))).collect();
        for round in 0.. {
            if pending.is_empty() {
                return 0;
            }
            let source: String = pending.iter().flat_map(|(_, asm)| [asm, "\n"]).collect();
            let errors = match self.assemble_object(&format!("reject{round}"), &source) {
                Ok(_) => BTreeMap::new(),
                Err(stderr) => Self::error_lines(&stderr),
            };
            if errors.is_empty() {
                break;
            }
            let refused: BTreeSet<usize> = errors.keys().map(|line| line - 1).collect();
            pending = pending
                .into_iter()
                .enumerate()
                .filter_map(|(index, entry)| (!refused.contains(&index)).then_some(entry))
                .collect();
        }
        for (name, asm) in pending.iter().take(REPORT_LIMIT) {
            writeln!(report, "  {name}: hod rejects the operands of `{asm}`, llvm-mc accepts it")
                .unwrap();
        }
        pending.len()
    }
}

/// Checks a disassembled mnemonic against the constructor's row in the
/// table, and against the case's own mnemonic when it has one.
fn mnemonic_ok(case: &Case, printed: &str) -> Result<(), String> {
    let allowed = base_mnemonics(case.name)
        .unwrap_or_else(|| panic!("constructor `{}` has no row in table.rs", case.name));
    if let Some(exact) = &case.mnemonic {
        let stem_ok = allowed
            .iter()
            .any(|stem| stem.strip_suffix('*').is_some_and(|stem| exact.starts_with(stem)));
        assert!(stem_ok, "`{exact}` is not in the `{}` family {allowed:?}", case.name);
        if printed == exact { Ok(()) } else { Err(format!("`{exact}`")) }
    } else {
        assert!(
            allowed.iter().all(|m| !m.ends_with('*')),
            "`{}` is a family: its cases must name the mnemonic",
            case.name
        );
        if allowed.contains(&printed) { Ok(()) } else { Err(format!("one of {allowed:?}")) }
    }
}

/// Adds a case: `emit!(batch, constructor(args), "asm {}", ...)`.
macro_rules! emit {
    ($b:expr, $f:ident($($a:expr),* $(,)?), $($fmt:tt)+) => {
        $b.push(stringify!($f), $f($($a),*), None, format!($($fmt)+))
    };
}

/// Adds a case for a constructor that covers a family of mnemonics:
/// `emit_as!(batch, mnemonic, constructor(args), "asm {}", ...)`.
macro_rules! emit_as {
    ($b:expr, $mn:expr, $f:ident($($a:expr),* $(,)?), $($fmt:tt)+) => {
        $b.push(stringify!($f), $f($($a),*), Some(String::from($mn)), format!($($fmt)+))
    };
}

/// Records operands the checked form must refuse, optionally with the
/// assembly text LLVM must refuse too:
/// `reject!(batch, try_constructor(args))` or
/// `reject!(batch, try_constructor(args), "asm {}", ...)`.
macro_rules! reject {
    ($b:expr, $f:ident($($a:expr),* $(,)?)) => {
        $b.reject(stringify!($f), $f($($a),*), &format!("{:?}", ($($a,)*)), None)
    };
    ($b:expr, $f:ident($($a:expr),* $(,)?), $($fmt:tt)+) => {
        $b.reject(stringify!($f), $f($($a),*), &format!("{:?}", ($($a,)*)), Some(format!($($fmt)+)))
    };
}
