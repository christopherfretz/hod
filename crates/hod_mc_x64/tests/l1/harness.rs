//! The L1 differential harness (`spec/mc-x64.md` "Test method").
//!
//! A [`Batch`] collects `(encoded bytes, assembly text)` pairs. Checking it
//! assembles every line with one `llvm-mc` run and compares each line's
//! bytes, found through a label before it; disassembles hod's own bytes
//! with `llvm-objdump` and compares the instruction boundaries and
//! mnemonics; and feeds the operands hod's checked forms rejected to
//! `llvm-mc`, which must reject each of them too. It does this once per
//! installed LLVM major.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use hod_mc_x64::Inst;
use hod_testutil::{LLVM_MAJORS, Scratch, run, tools_required};

use crate::table::base_mnemonics;

/// One encoding to check.
#[derive(Debug)]
pub struct Case {
    /// The constructor that produced the bytes (without any `try_` prefix).
    pub name: &'static str,
    pub bytes: Vec<u8>,
    /// The assembly line LLVM must encode to the same bytes. `$HERE` stands
    /// for a label at the start of the instruction.
    pub asm: String,
    /// The mnemonic LLVM must print for the bytes, when the constructor has
    /// more than one (it takes an operation-kind operand).
    pub mnemonic: Option<String>,
}

/// An operand combination a checked form refused.
#[derive(Debug)]
struct Rejection {
    name: &'static str,
    /// The assembly LLVM must refuse too, when the combination has a text
    /// form.
    asm: Option<String>,
}

/// A constructor result: an instruction, or the checked form's `Option`.
pub trait IntoInst {
    fn into_inst(self, asm: &str) -> Inst;
}

impl IntoInst for Inst {
    fn into_inst(self, _: &str) -> Inst {
        self
    }
}

impl IntoInst for Option<Inst> {
    fn into_inst(self, asm: &str) -> Inst {
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
        inst: impl IntoInst,
        mnemonic: Option<String>,
        asm: String,
    ) {
        let inst = inst.into_inst(&asm);
        self.push_bytes(name, inst.bytes().to_vec(), mnemonic, asm);
    }

    /// A case from raw bytes: the harness's own tests use it.
    pub fn push_bytes(
        &mut self,
        name: &'static str,
        bytes: Vec<u8>,
        mnemonic: Option<String>,
        asm: String,
    ) {
        assert!(!bytes.is_empty() && bytes.len() <= 15, "{asm}: {} bytes", bytes.len());
        self.cases.push(Case { name: base_name(name), bytes, asm, mnemonic });
    }

    /// Records that a checked form returned `None`, and fails at once if it
    /// did not.
    pub fn reject(
        &mut self,
        name: &'static str,
        result: Option<Inst>,
        operands: &str,
        asm: Option<String>,
    ) {
        assert!(
            result.is_none(),
            "{name}{operands} must be rejected, but encoded {:02x?}",
            result.map(|i| i.bytes().to_vec())
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

    /// Runs the differential checks against every installed LLVM major and
    /// panics with a report of every disagreement. Skips (after a note)
    /// when LLVM's tools are missing and not required.
    pub fn check(self, label: &str) {
        for tools in Tools::all() {
            let (mismatches, report) = self.examine(&tools);
            assert!(
                mismatches == 0,
                "{label}: {mismatches} disagreement(s) with LLVM {}\n{report}",
                tools.major
            );
            let with_asm = self.rejections.iter().filter(|r| r.asm.is_some()).count();
            println!(
                "l1 {label} (LLVM {}): {} encodings equal llvm-mc's and disassemble as \
                 intended; {} rejections, {with_asm} of them confirmed by llvm-mc",
                tools.major,
                self.cases.len(),
                self.rejections.len(),
            );
        }
    }
}

/// At most this many disagreements are listed per check.
const REPORT_LIMIT: usize = 40;

/// LLVM's tools of one major.
#[derive(Debug)]
pub struct Tools {
    pub major: u32,
    mc: PathBuf,
    objcopy: PathBuf,
    objdump: PathBuf,
    nm: PathBuf,
    scratch: Scratch,
}

const TRIPLE: &str = "--triple=x86_64-unknown-linux-gnu";

/// The first line of every source the harness assembles.
const HEADER: &str = ".intel_syntax noprefix\n";

/// `name-N` on `PATH`, or `/usr/lib/llvm-N/bin/name`.
fn versioned(name: &str, major: u32) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(format!("{name}-{major}")))
        .find(|p| p.is_file())
        .or_else(|| {
            let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/{name}"));
            installed.is_file().then_some(installed)
        })
}

impl Tools {
    /// The tools of `major`, if all four are installed.
    pub fn of(major: u32) -> Option<Self> {
        Some(Self {
            major,
            mc: versioned("llvm-mc", major)?,
            objcopy: versioned("llvm-objcopy", major)?,
            objdump: versioned("llvm-objdump", major)?,
            nm: versioned("llvm-nm", major)?,
            scratch: Scratch::new(&format!("mc-x64-{major}")),
        })
    }

    /// Every installed major, newest first. Empty, after a note, when none
    /// is installed; a failure then if tools are required.
    pub fn all() -> Vec<Self> {
        let found: Vec<Self> = LLVM_MAJORS.iter().filter_map(|&m| Self::of(m)).collect();
        if found.is_empty() {
            assert!(!tools_required(), "no LLVM {LLVM_MAJORS:?} tools found (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: LLVM's tools not found; skipping the tool-backed part of this test");
        }
        found
    }

    /// The newest installed major, for tests that need one oracle.
    pub fn find() -> Option<Self> {
        Self::all().into_iter().next()
    }

    pub fn scratch(&self) -> &Scratch {
        &self.scratch
    }

    pub fn objdump(&self) -> &Path {
        &self.objdump
    }

    /// Assembles `source` (Intel syntax; the header is added) to an object
    /// file. `Err` holds `llvm-mc`'s diagnostics.
    pub fn assemble_object(&self, stem: &str, source: &str) -> Result<PathBuf, String> {
        let src = self.scratch.write(&format!("{stem}.s"), format!("{HEADER}{source}"));
        let obj = self.scratch.join(format!("{stem}.o"));
        let outcome = run(
            &self.mc,
            [
                TRIPLE.as_ref(),
                "-filetype=obj".as_ref(),
                "-o".as_ref(),
                obj.as_os_str(),
                src.as_os_str(),
            ],
        );
        assert!(!outcome.timed_out(), "llvm-mc timed out on {stem}.s: an oracle ERROR");
        if outcome.success() { Ok(obj) } else { Err(outcome.stderr_text()) }
    }

    /// The bytes of an object's `.text`.
    pub fn text_bytes(&self, obj: &Path) -> Vec<u8> {
        let bin = obj.with_extension("bin");
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
        std::fs::read(&bin).expect("llvm-objcopy wrote its output")
    }

    /// The value of every defined symbol.
    pub fn symbols(&self, obj: &Path) -> BTreeMap<String, u64> {
        let outcome = run(&self.nm, ["--defined-only".as_ref(), obj.as_os_str()]);
        assert!(outcome.success(), "llvm-nm failed: {}", outcome.stderr_text());
        outcome
            .stdout_text()
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let value = u64::from_str_radix(parts.next()?, 16).ok()?;
                let _kind = parts.next()?;
                Some((parts.next()?.to_string(), value))
            })
            .collect()
    }

    /// Disassembles `.text` of `obj`: `(offset, instruction text)` per
    /// instruction, a prefix printed on a line of its own (`lock`) joined
    /// to the instruction after it.
    pub fn disassemble(&self, obj: &Path) -> Vec<(u64, String)> {
        let outcome = run(
            &self.objdump,
            [
                "-d".as_ref(),
                "--no-show-raw-insn".as_ref(),
                "-M".as_ref(),
                "intel".as_ref(),
                obj.as_os_str(),
            ],
        );
        assert!(outcome.success(), "llvm-objdump failed: {}", outcome.stderr_text());
        let mut out: Vec<(u64, String)> = Vec::new();
        let mut pending: Option<(u64, String)> = None;
        for line in outcome.stdout_text().lines() {
            let Some((address, rest)) = line.split_once(':') else { continue };
            let address = address.trim();
            if address.is_empty() || !address.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            let Ok(offset) = u64::from_str_radix(address, 16) else { continue };
            let text = rest.split_whitespace().collect::<Vec<_>>().join(" ");
            let text = match pending.take() {
                Some((start, prefix)) => {
                    out.push((start, format!("{prefix} {text}")));
                    continue;
                }
                None => text,
            };
            if PREFIXES.contains(&text.as_str()) {
                pending = Some((offset, text));
            } else {
                out.push((offset, text));
            }
        }
        if let Some(p) = pending {
            out.push(p);
        }
        out
    }

    /// The 1-based case numbers `llvm-mc` reported an error on (source
    /// line minus the header), with the first message for each.
    fn error_lines(stderr: &str) -> BTreeMap<usize, String> {
        let mut lines = BTreeMap::new();
        for line in stderr.lines() {
            let Some((location, message)) = line.split_once(": error: ") else { continue };
            let mut parts = location.rsplitn(3, ':');
            let (_column, Some(number)) = (parts.next(), parts.next()) else { continue };
            if let Ok(number) = number.parse::<usize>()
                && number > 1
            {
                lines.entry(number - 1).or_insert_with(|| message.to_string());
            }
        }
        lines
    }

    /// Forward direction: every assembly line encodes to hod's bytes.
    fn compare(&self, cases: &[Case], report: &mut String) -> usize {
        if cases.is_empty() {
            return 0;
        }
        let mut source = String::new();
        for (i, case) in cases.iter().enumerate() {
            assert!(!case.asm.contains('\n'), "one instruction per case");
            let label = format!("h{i}");
            writeln!(source, "{label}: {}", case.asm.replace("$HERE", &label)).unwrap();
        }
        let obj = match self.assemble_object("forward", &source) {
            Ok(obj) => obj,
            Err(stderr) => {
                let errors = Self::error_lines(&stderr);
                for (line, message) in errors.iter().take(REPORT_LIMIT) {
                    let case = &cases[line - 1];
                    writeln!(
                        report,
                        "  {}: llvm-mc refuses `{}` (hod: {:02x?}): {message}",
                        case.name, case.asm, case.bytes
                    )
                    .unwrap();
                }
                if errors.is_empty() {
                    writeln!(report, "  llvm-mc failed: {stderr}").unwrap();
                }
                return errors.len().max(1);
            }
        };
        let text = self.text_bytes(&obj);
        let symbols = self.symbols(&obj);
        let start = |i: usize| -> usize {
            *symbols.get(&format!("h{i}")).unwrap_or_else(|| panic!("label h{i}")) as usize
        };
        let mut mismatches = 0;
        for (i, case) in cases.iter().enumerate() {
            let from = start(i);
            let to = if i + 1 < cases.len() { start(i + 1) } else { text.len() };
            let llvm = &text[from..to];
            if case.bytes != llvm {
                mismatches += 1;
                if mismatches <= REPORT_LIMIT {
                    writeln!(
                        report,
                        "  {}: `{}`: hod {:02x?}, llvm-mc {llvm:02x?}",
                        case.name, case.asm, case.bytes
                    )
                    .unwrap();
                }
            }
        }
        mismatches
    }

    /// Reverse direction: hod's bytes disassemble, instruction for
    /// instruction, to the instruction the constructor is specified to
    /// produce.
    fn reverse(&self, cases: &[Case], report: &mut String) -> usize {
        if cases.is_empty() {
            return 0;
        }
        let mut source = String::new();
        for case in cases {
            let bytes: Vec<String> = case.bytes.iter().map(|b| format!("{b:#04x}")).collect();
            writeln!(source, ".byte {}", bytes.join(",")).unwrap();
        }
        let obj = self
            .assemble_object("reverse", &source)
            .unwrap_or_else(|e| panic!("llvm-mc cannot assemble .byte lines: {e}"));
        let decoded: BTreeMap<u64, String> = self.disassemble(&obj).into_iter().collect();
        let mut mismatches = 0;
        let mut offset = 0u64;
        for case in cases {
            let end = offset + case.bytes.len() as u64;
            let printed = decoded.get(&offset);
            let next = decoded.range(offset + 1..).next().map(|(o, _)| *o);
            let verdict = match printed {
                None => Err("no instruction starts here".to_string()),
                Some(_) if next.is_some_and(|n| n != end) => Err(format!(
                    "the decoded instruction is {} bytes, not {}",
                    next.unwrap_or(end) - offset,
                    case.bytes.len()
                )),
                Some(text) => mnemonic_ok(case, text),
            };
            if let Err(expected) = verdict {
                mismatches += 1;
                if mismatches <= REPORT_LIMIT {
                    writeln!(
                        report,
                        "  {}: {:02x?} (`{}`) disassembles as `{}`: {expected}",
                        case.name,
                        case.bytes,
                        case.asm,
                        printed.map_or("", String::as_str)
                    )
                    .unwrap();
                }
            }
            offset = end;
        }
        mismatches
    }

    /// Every rejection with assembly text is an error to LLVM as well.
    ///
    /// `llvm-mc` reports operand errors while parsing and fixup errors only
    /// if parsing succeeded, so lines without an error are retried until a
    /// run reports nothing new.
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

/// Prefixes `llvm-objdump` prints on a line of their own.
const PREFIXES: [&str; 1] = ["lock"];

/// The mnemonic of a disassembled instruction: its first word, or its
/// first two when the first is a prefix.
fn mnemonic_of(text: &str) -> String {
    let mut words = text.split_whitespace();
    let first = words.next().unwrap_or("");
    if PREFIXES.contains(&first) {
        format!("{first} {}", words.next().unwrap_or(""))
    } else {
        first.to_string()
    }
}

/// Checks a disassembled instruction's mnemonic against the constructor's
/// row in the table, and against the case's own mnemonic when it has one.
fn mnemonic_ok(case: &Case, text: &str) -> Result<(), String> {
    let printed = mnemonic_of(text);
    let allowed = base_mnemonics(case.name)
        .unwrap_or_else(|| panic!("constructor `{}` has no row in table.rs", case.name));
    if let Some(exact) = &case.mnemonic {
        let stem_ok = allowed
            .iter()
            .any(|stem| stem.strip_suffix('*').is_some_and(|stem| exact.starts_with(stem)));
        assert!(stem_ok, "`{exact}` is not in the `{}` family {allowed:?}", case.name);
        if printed == *exact { Ok(()) } else { Err(format!("expected `{exact}`")) }
    } else {
        assert!(
            allowed.iter().all(|m| !m.ends_with('*')),
            "`{}` is a family: its cases must name the mnemonic",
            case.name
        );
        if allowed.contains(&printed.as_str()) {
            Ok(())
        } else {
            Err(format!("expected one of {allowed:?}"))
        }
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
