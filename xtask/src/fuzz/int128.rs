//! The `llc19-int128-last-register` class (`spec/harness.md` "Set apart
//! (seeds)"): a finding of the `abi` profile at LLVM 19 on x86-64 that
//! `llc` 19's own call lowering explains, set apart only on evidence
//! gathered for that finding.
//!
//! The x86-64 psABI passes an `__int128` argument that does not fit in
//! the integer registers left (only `r9`) wholly on the stack, and the
//! next integer argument still gets `r9`: gcc, `llc` 22 and hod do that;
//! `llc` 19 leaves `r9` unused, so its caller and a callee built by
//! anyone else disagree (the 1.0 budget at df415453: 21 findings).
//!
//! A finding is set apart when (1) it is a `disagree` of the `abi`
//! profile at LLVM 19 on x86-64, (2) the module has the shape: a call or
//! a definition with an `i128` argument where exactly one of the six
//! integer argument registers is left ([`last_register_site`]), and (3)
//! the finding's own program built by gcc `-O0` from `prog.c` and the
//! finding's own module built by `llc` 22 (the oracle's flags) both print
//! exactly what hod's program printed and end the same way. In `--mix`,
//! hod's program is the finding's assignment with `llc` 22 building the
//! other half (the half `llc` 19 built in the finding). Anything else
//! leaves the finding a finding, with the reason added to its detail
//! when the shape was there.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use super::case::{Bench, Files, Finding, Kind, Outcome, Side, Verdict};
use crate::irdiff::Arch;
use crate::irdiff::process::{End, Output};

/// The class's name, in the results, the summary and the budget.
pub const LLC19_INT128: &str = "llc19-int128-last-register";

/// The LLVM whose `llc` has the defect.
pub const DEFECT_LLVM: u32 = 19;

/// The LLVM whose `llc` is asked as well (the oracle's flags).
pub const NEWER_LLVM: u32 = 22;

/// The integer argument registers of the x86-64 psABI (rdi, rsi, rdx,
/// rcx, r8, r9).
const GP_REGISTERS: usize = 6;

/// Whether the class can apply to a run at all: the `abi` profile, x86-64,
/// LLVM 19.
pub fn applies(arch: Arch, llvm: u32, profile: &str) -> bool {
    arch == Arch::X86_64 && llvm == DEFECT_LLVM && profile == "abi"
}

/// How an argument of a call or a definition uses the integer registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    /// One register (a pointer, an integer of at most 64 bits).
    One,
    /// `i128`: two registers, or the stack when fewer are left.
    Int128,
    /// None (floating point, vectors, `byval` memory).
    None,
    /// `...` in a definition: the fixed parameters end.
    End,
}

/// How `arg` (one argument, `<type> <attributes> <value>`) uses the
/// integer registers; `None` when the type is not one this knows (the
/// site is then not judged).
fn use_of(arg: &str) -> Option<Use> {
    let arg = arg.trim();
    if arg == "..." {
        return Some(Use::End);
    }
    let ty = arg.split_whitespace().next()?;
    if arg.contains(" byval(") || arg.contains(" byval ") {
        return Some(Use::None);
    }
    match ty {
        "ptr" => Some(Use::One),
        "i128" => Some(Use::Int128),
        "half" | "bfloat" | "float" | "double" | "fp128" | "x86_fp80" => Some(Use::None),
        _ if ty.starts_with('<') && !ty.starts_with("<{") => Some(Use::None),
        _ => {
            let bits: u32 = ty.strip_prefix('i')?.parse().ok()?;
            (bits <= 64).then_some(Use::One)
        }
    }
}

/// `text` cut at its top-level commas (inside no `()`, `{}`, `[]`,
/// `<>`).
fn split_top(text: &str) -> Vec<&str> {
    let (mut depth, mut start, mut parts) = (0i32, 0, Vec::new());
    for (i, c) in text.char_indices() {
        match c {
            '(' | '{' | '[' | '<' => depth += 1,
            ')' | '}' | ']' | '>' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if !text[start..].trim().is_empty() {
        parts.push(&text[start..]);
    }
    parts
}

/// The callee's (or the defined function's) name and its argument list:
/// the first `(` right after a word that starts with `@` or `%`, to its
/// matching `)`.
fn site_of(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut search = 0;
    while let Some(at) = line[search..].find('(').map(|i| i + search) {
        let word_start = line[..at]
            .rfind(|c: char| c.is_whitespace() || c == '(' || c == ',')
            .map_or(0, |i| i + 1);
        let word = &line[word_start..at];
        if word.len() > 1 && (word.starts_with('@') || word.starts_with('%')) {
            let mut depth = 0i32;
            for (i, &b) in bytes.iter().enumerate().skip(at) {
                match b {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some((word, &line[at + 1..i]));
                        }
                    }
                    _ => {}
                }
            }
            return None;
        }
        search = at + 1;
    }
    None
}

/// The first call or definition of `text` (a module for x86-64) with an
/// `i128` argument at a point where exactly one integer argument register
/// is left, as `call @f argument N` or `define @f parameter N` (counted
/// from 1). `None`: no such site, or none whose arguments' types are all
/// known.
pub fn last_register_site(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim_start();
        let what = if trimmed.starts_with("define ") {
            "define"
        } else if trimmed.contains("call ") && !trimmed.starts_with(';') {
            "call"
        } else {
            continue;
        };
        let Some((name, args)) = site_of(trimmed) else { continue };
        let mut used = 0;
        for (index, arg) in split_top(args).into_iter().enumerate() {
            match use_of(arg) {
                Some(Use::One) => used += 1,
                Some(Use::Int128) if used + 1 == GP_REGISTERS => {
                    let noun = if what == "define" { "parameter" } else { "argument" };
                    return Some(format!("{what} {name} {noun} {}", index + 1));
                }
                Some(Use::Int128) if used + 2 <= GP_REGISTERS => used += 2,
                Some(Use::Int128 | Use::None) => {}
                Some(Use::End) | None => break,
            }
        }
    }
    None
}

/// The evidence of a finding set apart as [`LLC19_INT128`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Int128SetApart {
    /// The site of the shape ([`last_register_site`]).
    pub site: String,
    /// hod's program: `hod` or the `--mix` assignment with `llc` 22.
    pub program: String,
    /// The bytes all three printed, and how they ended.
    pub bytes: usize,
    pub end: End,
    /// The finding's detail (how `llc` 19's program differs).
    pub finding: String,
}

impl Int128SetApart {
    /// The line of the summary and the budget's log.
    pub fn line(&self, seed: u64) -> String {
        format!(
            "{LLC19_INT128}: seed {seed}: i128 at {} with one integer register left; gcc -O0 \
             (prog.c) and llc-{NEWER_LLVM} -O0 (the module) print what {} printed ({} bytes, {}); \
             llc-{DEFECT_LLVM}: {}",
            self.site,
            self.program,
            self.bytes,
            super::case::ended(self.end),
            self.finding
        )
    }
}

/// The decision, from the evidence gathered: `Ok` sets the finding apart,
/// `Err` says why it stays a finding. `gcc` and `newer` are the programs'
/// outputs, or why there is none.
pub fn decide(
    site: &str,
    finding: &Finding,
    hod: (&str, &Output),
    gcc: Result<&Output, &str>,
    newer: Result<&Output, &str>,
) -> Result<Int128SetApart, String> {
    let same = |other: &Output| other.end == hod.1.end && other.stdout == hod.1.stdout;
    if finding.kind != Kind::Disagree {
        return Err(format!("a {} finding, not disagree", finding.kind.name()));
    }
    if hod.1.end == End::TimedOut {
        return Err(format!("{} timed out", hod.0));
    }
    for (who, output) in [("gcc -O0", gcc), (&*format!("llc-{NEWER_LLVM}"), newer)] {
        match output {
            Ok(output) if same(output) => {}
            Ok(_) => return Err(format!("{who}'s program differs from {}'s", hod.0)),
            Err(why) => return Err(format!("no evidence from {who}: {why}")),
        }
    }
    Ok(Int128SetApart {
        site: site.to_owned(),
        program: hod.0.to_owned(),
        bytes: hod.1.stdout.len(),
        end: hod.1.end,
        finding: finding.detail.clone(),
    })
}

/// The halves of a `--mix` finding: hod's object of the side that built
/// it, and the module of the other half (the one `llc` 19 built).
#[derive(Clone, Copy, Debug)]
pub struct MixSide<'a> {
    pub side: Side,
    pub hod_object: &'a Path,
    pub other_half: &'a Path,
}

impl Bench {
    /// [`LLC19_INT128`] for `verdict` (a seed's verdict, with `outcome.hod`
    /// hod's program's output in whole mode): the verdict set apart, or
    /// unchanged. The builds run only for a finding with the shape.
    pub fn set_apart_int128(
        &self,
        verdict: Verdict,
        files: &Files,
        outcome: &mut Outcome,
        mix: Option<MixSide<'_>>,
    ) -> Verdict {
        let Verdict::Finding(mut finding) = verdict else { return verdict };
        if !applies(self.arch, self.tools.llvm_major, self.profile.name)
            || finding.kind != Kind::Disagree
        {
            return Verdict::Finding(finding);
        }
        let text = fs::read_to_string(files.ir()).unwrap_or_default();
        let Some(site) = last_register_site(&text) else { return Verdict::Finding(finding) };
        match self.int128_evidence(&site, &finding, files, outcome, mix) {
            Ok(apart) => {
                outcome.commands.push(format!("# {}", apart.line(outcome.seed)));
                outcome.int128 = Some(apart);
                Verdict::SetApart(LLC19_INT128)
            }
            Err(why) => {
                let _ = write!(finding.detail, "; not {LLC19_INT128}: {why}");
                outcome.commands.push(format!("# not {LLC19_INT128}: {why}"));
                Verdict::Finding(finding)
            }
        }
    }

    /// Builds and runs gcc's program, `llc` 22's, and in `--mix` hod's
    /// half with `llc` 22's other half; [`decide`]s.
    fn int128_evidence(
        &self,
        site: &str,
        finding: &Finding,
        files: &Files,
        outcome: &mut Outcome,
        mix: Option<MixSide<'_>>,
    ) -> Result<Int128SetApart, String> {
        let newer_llc =
            self.tools.newer_llc.as_ref().ok_or(format!("llc-{NEWER_LLVM} not found"))?;
        let log = &mut outcome.commands;
        let gcc = self.int128_gcc(log, files);
        let newer_o = files.at(&format!("llc{NEWER_LLVM}.o"));
        let newer = self.build_with(log, newer_llc, &files.ir(), &newer_o).and_then(|()| {
            self.run_objects(log, &[&newer_o], &files.at(&format!("llc{NEWER_LLVM}")))
        });
        let mixed;
        let hod = match &mix {
            None => ("hod", outcome.hod.clone().ok_or("no output of hod's program")?),
            Some(mix) => {
                let other = files.at(&format!("other.llc{NEWER_LLVM}.o"));
                self.build_with(log, newer_llc, mix.other_half, &other)?;
                let objects: [&Path; 2] = if mix.side == Side::HodA {
                    [mix.hod_object, &other]
                } else {
                    [&other, mix.hod_object]
                };
                mixed = match mix.side {
                    Side::HodA => format!("hod A + llc-{NEWER_LLVM} B"),
                    _ => format!("llc-{NEWER_LLVM} A + hod B"),
                };
                let output = self.run_objects(log, &objects, &files.at("mix-newer"))?;
                (mixed.as_str(), output)
            }
        };
        decide(
            site,
            finding,
            (hod.0, &hod.1),
            gcc.as_ref().map_err(String::as_str),
            newer.as_ref().map_err(String::as_str),
        )
    }

    /// `ir` built by `llc` (the oracle's flags) into `object`.
    fn build_with(
        &self,
        log: &mut Vec<String>,
        llc: &Path,
        ir: &Path,
        object: &Path,
    ) -> Result<(), String> {
        let output =
            self.step(log, llc, &self.reference_args(ir, object), None, self.tool_limits)?;
        match output.end {
            End::Exit(0) if object.is_file() => Ok(()),
            end => Err(format!("llc {}: {}", super::case::ended(end), output.first_error_line())),
        }
    }

    /// `objects` linked into `program` and run.
    fn run_objects(
        &self,
        log: &mut Vec<String>,
        objects: &[&Path],
        program: &Path,
    ) -> Result<Output, String> {
        self.link_all(log, objects, program)?;
        self.execute(log, program, self.run_limits).map(|(output, _)| output)
    }

    /// `prog.c` built by the target's gcc at `-O0`, linked as the
    /// oracle is, and run.
    fn int128_gcc(&self, log: &mut Vec<String>, files: &Files) -> Result<Output, String> {
        let program: PathBuf = files.at("gcc");
        let mut args = Vec::new();
        if let Some(dir) = &self.tools.linker_dir {
            args.push(crate::irdiff::tools::prefixed("-B", dir));
        }
        args.extend(["-O0", "-w", "-pie"].map(Into::into));
        args.extend(
            crate::irdiff::tools::linker_flags(&self.tools.linker_name)
                .iter()
                .map(std::ffi::OsString::from),
        );
        args.push("-o".into());
        args.push(program.clone().into());
        args.push(files.source().into());
        let output = self.step(log, &self.tools.driver, &args, None, self.tool_limits)?;
        if output.end != End::Exit(0) || !program.is_file() {
            return Err(format!(
                "gcc {}: {}",
                super::case::ended(output.end),
                output.first_error_line()
            ));
        }
        self.execute(log, &program, self.run_limits).map(|(output, _)| output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(end: End, stdout: &[u8]) -> Output {
        Output { end, stdout: stdout.to_vec(), stderr: Vec::new(), truncated: false }
    }

    // The call of seed 125392 (the 1.0 budget): sret, i32, i64, i64, i32
    // take five registers; the variadic i128 comes with only r9 left.
    const CALL_125392: &str = "  call void (ptr, float, i32, double, i64, i64, float, i32, ...) \
        @callee_2(ptr dead_on_unwind writable sret(%struct.A4) align 8 %1, float noundef \
        0x40BA9C8C00000000, i32 noundef 2037361584, double noundef 0x405CD0A800000000, i64 \
        noundef %4, i64 noundef %6, float noundef 0x40D7F6B400000000, i32 noundef -14, x86_fp80 \
        noundef 0xK400FAE2C600000000000, i128 noundef 1606, double noundef 1.0, i64 noundef 15)";

    // spec/harness.md "Set apart (seeds)": the shape.
    #[test]
    fn the_shape() {
        assert_eq!(last_register_site(CALL_125392).as_deref(), Some("call @callee_2 argument 10"));
        let define = "define dso_local void @f(ptr noundef %0, i32 noundef %1, i64 %2, i8 \
                      zeroext %3, i16 signext %4, i128 noundef %5) #0 {";
        assert_eq!(last_register_site(define).as_deref(), Some("define @f parameter 6"));
        // Two registers left: the i128 fits; none left: the stack, and so
        // nothing changes for the next argument.
        for no in [
            "  call void @f(i32 1, i32 2, i32 3, i32 4, i128 5, i32 6)",
            "  call void @f(i32 1, i32 2, i32 3, i32 4, i32 5, i32 6, i128 7, i32 8)",
            "  call void @f(i128 1, i128 2, i128 3)",
            // byval and floating point take no integer register.
            "  call void @f(ptr byval(%struct.S) align 8 %0, i32 1, i32 2, i32 3, i32 4, \
             double 1.0, <2 x float> %v, i128 9)",
            // After `...` in a definition nothing is fixed.
            "define void @g(i32 %0, i32 %1, i32 %2, i32 %3, i32 %4, ...) {",
            // A type this does not know: not judged.
            "  call void @f(i32 1, i32 2, { i64, i64 } %s, i32 4, i32 5, i128 6)",
            // No call, no definition.
            "@g = global i128 5, align 16",
            "; call void @f(i32 1, i32 2, i32 3, i32 4, i32 5, i128 6)",
        ] {
            assert_eq!(last_register_site(no), None, "{no}");
        }
        assert_eq!(
            last_register_site(
                "  %r = call i32 (ptr, ...) %fp(ptr %a, i32 1, i32 2, i32 3, i32 4, i128 %w)"
            )
            .as_deref(),
            Some("call %fp argument 6")
        );
    }

    // spec/harness.md "Set apart (seeds)": only the abi profile at LLVM 19
    // on x86-64.
    #[test]
    fn where_it_applies() {
        assert!(applies(Arch::X86_64, 19, "abi"));
        assert!(!applies(Arch::X86_64, 22, "abi"));
        assert!(!applies(Arch::Aarch64, 19, "abi"));
        assert!(!applies(Arch::X86_64, 19, "default"));
    }

    // spec/harness.md "Set apart (seeds)": set apart only when gcc and
    // llc 22 both print hod's output.
    #[test]
    fn the_decision() {
        let finding = Finding {
            kind: Kind::Disagree,
            detail: "stdout differs at byte 252 (line 5); both exited with status 0".to_owned(),
            side: Side::Whole,
        };
        let hod = output(End::Exit(0), b"1 15 7\n");
        let other = output(End::Exit(0), b"1 -1 15\n");
        let site = "call @callee_2 argument 10";
        let apart = decide(site, &finding, ("hod", &hod), Ok(&hod), Ok(&hod)).unwrap();
        assert_eq!((apart.bytes, apart.end), (7, End::Exit(0)));
        let line = apart.line(125_392);
        assert!(
            line.starts_with("llc19-int128-last-register: seed 125392: i128 at call"),
            "{line}"
        );
        assert!(line.contains("print what hod printed (7 bytes, exited with status 0)"), "{line}");
        // One of the two differs, or gives no evidence: a finding.
        assert!(decide(site, &finding, ("hod", &hod), Ok(&other), Ok(&hod)).is_err());
        assert!(decide(site, &finding, ("hod", &hod), Ok(&hod), Ok(&other)).is_err());
        let ended = output(End::Exit(1), b"1 15 7\n");
        assert!(decide(site, &finding, ("hod", &hod), Ok(&ended), Ok(&hod)).is_err());
        let why = decide(site, &finding, ("hod", &hod), Ok(&hod), Err("llc failed")).unwrap_err();
        assert_eq!(why, "no evidence from llc-22: llc failed");
        // Not a disagreement, or hod's program did not end.
        let hang = Finding { kind: Kind::HodHang, ..finding.clone() };
        assert!(decide(site, &hang, ("hod", &hod), Ok(&hod), Ok(&hod)).is_err());
        let late = output(End::TimedOut, b"");
        assert!(decide(site, &finding, ("hod", &late), Ok(&late), Ok(&late)).is_err());
    }
}
