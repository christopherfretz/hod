//! `--mix`: the ABI between hod's objects and `llc`'s
//! (`spec/harness.md` "Random programs", "Mix").
//!
//! The module is split in two halves by `llvm-extract`: half A is what
//! [`plan`] lists (each defined function and global by a seeded hash of
//! its name), half B the rest (`--delete` of the same list). What one half
//! uses of the other becomes a declaration there. `llvm-extract` gives
//! every definition of local linkage (csmith's `static` functions and
//! globals, clang's `private` strings) external linkage and hidden
//! visibility, in both halves, and the declarations it writes are hidden
//! too: names are unique in a module, so each local becomes one symbol of
//! the program that only its two objects see, and the halves together
//! mean what the module meant. The run checks that claim on every seed:
//! `llc`'s two halves linked together must agree with the oracle, or the
//! seed is an error of the harness's side.
//!
//! hod builds A with `llc`'s B, and B with `llc`'s A; each program is
//! compared with the oracle (the whole module built by `llc`).

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use super::case::{Bench, Files, Finding, Kind, Outcome, Side, Verdict, compare};
use crate::irdiff::process::{End, Output};

/// FNV-1a of `name`, mixed with the seed.
fn hash(seed: u64, name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let mut z = h ^ seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Whether the definition `name` goes to half A for `seed`. A function of
/// the name, so that a reduction, which deletes definitions, keeps the
/// others where they were.
pub fn in_a(seed: u64, name: &str) -> bool {
    hash(seed, name) & 1 == 1
}

/// What half A gets: `(function?, name)` of each definition, in the
/// module's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub a: Vec<(bool, String)>,
    /// Definitions in half B.
    pub b: usize,
}

/// The definitions of `text` (functions and global variables; not the
/// `llvm.` globals, not names in quotes) and the half of each.
pub fn plan(text: &str, seed: u64) -> Plan {
    let mut plan = Plan::default();
    for line in text.lines() {
        let definition = if let Some(header) = line.strip_prefix("define ") {
            header
                .split_once(" @")
                .and_then(|(_, rest)| rest.split_once('('))
                .map(|(n, _)| (true, n))
        } else if let Some(rest) = line.strip_prefix('@') {
            rest.split_once(" = ")
                .filter(|(_, value)| {
                    let value = value.trim_start();
                    !value.starts_with("external ")
                        && !value.contains(" alias ")
                        && !value.starts_with("alias ")
                        && !value.contains(" ifunc ")
                })
                .map(|(n, _)| (false, n))
        } else {
            None
        };
        let Some((function, name)) = definition else { continue };
        let simple = !name.is_empty()
            && !name.starts_with("llvm.")
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._$-".contains(&b));
        if !simple {
            continue;
        }
        if in_a(seed, name) {
            plan.a.push((function, name.to_owned()));
        } else {
            plan.b += 1;
        }
    }
    plan
}

impl Plan {
    /// `llvm-extract`'s arguments: half A (`delete` false) or half B.
    pub fn extract_args(&self, ir: &Path, out: &Path, delete: bool) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec!["-S".into()];
        if delete {
            args.push("--delete".into());
        }
        for (function, name) in &self.a {
            args.push(format!("--{}={name}", if *function { "func" } else { "glob" }).into());
        }
        args.push("-o".into());
        args.push(out.into());
        args.push(ir.into());
        args
    }
}

/// The two halves' files.
#[derive(Debug)]
pub struct Halves {
    pub a: std::path::PathBuf,
    pub b: std::path::PathBuf,
}

impl Bench {
    /// Splits `ir` into `<prefix>a.ll` and `<prefix>b.ll` in `dir`.
    /// `Ok(None)`: one half would have no definition.
    pub fn split(
        &self,
        log: &mut Vec<String>,
        seed: u64,
        ir: &Path,
        dir: &Path,
        prefix: &str,
    ) -> Result<Option<Halves>, String> {
        let text =
            fs::read_to_string(ir).map_err(|e| format!("cannot read '{}': {e}", ir.display()))?;
        let plan = plan(&text, seed);
        if plan.a.is_empty() || plan.b == 0 {
            return Ok(None);
        }
        let extract = self.tools.extract.as_ref().ok_or("llvm-extract not found")?;
        let halves =
            Halves { a: dir.join(format!("{prefix}a.ll")), b: dir.join(format!("{prefix}b.ll")) };
        for (out, delete) in [(&halves.a, false), (&halves.b, true)] {
            let output = self.step(
                log,
                extract,
                &plan.extract_args(ir, out, delete),
                None,
                self.tool_limits,
            )?;
            if output.end != End::Exit(0) || !out.is_file() {
                return Err(format!("llvm-extract failed: {}", output.first_error_line()));
            }
        }
        Ok(Some(halves))
    }

    /// The steps of a `--mix` seed after clang ("Mix").
    pub fn mix_steps(
        &self,
        seed: u64,
        files: &Files,
        outcome: &mut Outcome,
    ) -> Result<Verdict, Verdict> {
        let log = &mut outcome.commands;
        let harness = |e: String| Verdict::Error(format!("harness: {e}"));
        let ir = files.ir();
        let oracle_o = files.at("oracle.o");
        self.build_with_llc(log, &ir, &oracle_o, false).map_err(Verdict::Error)?;
        let oracle_x = files.at("oracle");
        self.link(log, &oracle_o, &oracle_x)
            .map_err(|e| Verdict::Error(format!("the oracle's link: {e}")))?;
        let (oracle, oracle_time) =
            self.execute(log, &oracle_x, self.run_limits).map_err(harness)?;
        if oracle.end == End::TimedOut {
            return Err(Verdict::Skipped("oracle-timed-out"));
        }
        outcome.oracle_time = Some(oracle_time);
        let Some(halves) = self.split(log, seed, &ir, &files.dir, "").map_err(harness)? else {
            return Err(Verdict::Skipped("nothing-to-split"));
        };
        // llc's halves together must be the oracle's program.
        let (llc_a, llc_b) = (files.at("a.llc.o"), files.at("b.llc.o"));
        self.build_with_llc(log, &halves.a, &llc_a, false).map_err(Verdict::Error)?;
        self.build_with_llc(log, &halves.b, &llc_b, false).map_err(Verdict::Error)?;
        let split_x = files.at("split");
        self.link_all(log, &[&llc_a, &llc_b], &split_x)
            .map_err(|e| Verdict::Error(format!("the link of llc's halves: {e}")))?;
        let (split, _) = self.execute(log, &split_x, self.run_limits).map_err(harness)?;
        if split.end == End::TimedOut {
            return Err(Verdict::Skipped("split-timed-out"));
        }
        if let Verdict::Finding(differs) = compare(&oracle, &split) {
            return Err(Verdict::Error(format!(
                "harness: llc's two halves disagree with the whole module: {}",
                differs.detail
            )));
        }
        let (hod_a, hod_b) = (files.at("a.hod.o"), files.at("b.hod.o"));
        for (half, ll, object) in [("A", &halves.a, &hod_a), ("B", &halves.b, &hod_b)] {
            let coverage = files.at(&format!("coverage-{half}.tsv"));
            match self.build_with_hod(log, ll, object, &coverage) {
                Ok(Ok(())) => {}
                Ok(Err(reason)) => return Err(Verdict::Refused(reason)),
                Err(detail) => {
                    let side = if half == "A" { Side::HodA } else { Side::HodB };
                    return Err(mixed(Kind::HodFailed, side, format!("half {half}: {detail}")));
                }
            }
        }
        let mut first: Option<(Finding, Option<Output>)> = None;
        let mut agreed = Vec::new();
        for (side, objects, name) in
            [(Side::HodA, [&hod_a, &llc_b], "mix-a"), (Side::HodB, [&llc_a, &hod_b], "mix-b")]
        {
            let program = files.at(name);
            if let Err(e) = self.link_all(log, &[objects[0], objects[1]], &program) {
                let found = Finding {
                    kind: Kind::HodLink,
                    detail: format!("{}: the objects do not link: {e}", side.name()),
                    side,
                };
                first.get_or_insert((found, None));
                continue;
            }
            let what = format!("the program of {}", side.name());
            let (verdict, output) = self.judge(log, &oracle, oracle_time, &program, &what)?;
            match verdict {
                Verdict::Finding(mut found) => {
                    found.side = side;
                    if found.kind != Kind::HodHang {
                        found.detail = format!("{}: {}", side.name(), found.detail);
                    }
                    first.get_or_insert((found, Some(output)));
                }
                Verdict::Agree => agreed.push(side.name()),
                other => return Ok(other),
            }
        }
        let Some((mut found, output)) = first else { return Ok(Verdict::Agree) };
        let _ = write!(
            found.detail,
            "; {}",
            if agreed.is_empty() {
                "the other assignment disagrees too".to_owned()
            } else {
                format!("{} agrees", agreed.join(", "))
            }
        );
        outcome.oracle = Some(oracle);
        outcome.hod = output;
        Ok(Verdict::Finding(found))
    }
}

fn mixed(kind: Kind, side: Side, detail: String) -> Verdict {
    Verdict::Finding(Finding { kind, detail, side })
}

/// The second witness of a `--mix` finding: hod's whole module against
/// the oracle. When it agrees and the mixed program does not, hod and
/// `llc` disagree about the interface between objects.
pub fn whole_witness(bench: &Bench, dir: &Path, ir: &Path, oracle: &Output) -> String {
    let mut log = Vec::new();
    let (object, program) = (dir.join("whole-hod.o"), dir.join("whole-hod"));
    match bench.build_with_hod(&mut log, ir, &object, &dir.join("whole-coverage.tsv")) {
        Ok(Ok(())) => {}
        Ok(Err(reason)) => return format!("hod declines the whole module: {reason}"),
        Err(e) => return format!("hod fails on the whole module: {e}"),
    }
    if let Err(e) = bench.link(&mut log, &object, &program) {
        return format!("hod's whole object does not link: {e}");
    }
    match bench.execute(&mut log, &program, bench.run_limits) {
        Ok((whole, _)) if whole.end == oracle.end && whole.stdout == oracle.stdout => {
            "hod's whole module agrees with the oracle: hod and llc disagree between objects"
                .to_owned()
        }
        Ok((whole, _)) => format!(
            "hod's whole module disagrees with the oracle too ({})",
            match compare(oracle, &whole) {
                Verdict::Finding(f) => f.detail,
                _ => String::new(),
            }
        ),
        Err(e) => format!("hod's whole program did not run: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec/harness.md "Mix": what each half gets.
    #[test]
    fn plans_by_name() {
        let module = "\
@.str = private unnamed_addr constant [2 x i8] c\"1\\00\", align 1
@g_2 = internal global i64 1, align 8
@e = external global i32
@llvm.used = appending global [0 x ptr] zeroinitializer
@\"q u\" = global i8 0
define dso_local i32 @main(i32 noundef %0) #0 {
define internal { i32, i8 } @f(i32 %a) #0 {
declare i32 @printf(ptr noundef, ...) #2
";
        let names: Vec<String> =
            ["@.str", "@g_2", "@main", "@f"].iter().map(|n| n[1..].to_owned()).collect();
        let mut seen_a = [false; 4];
        let mut seen_b = [false; 4];
        for seed in 0..64 {
            let plan = plan(module, seed);
            assert_eq!(plan.a.len() + plan.b, 4, "{plan:?}");
            for (i, name) in names.iter().enumerate() {
                let a = plan.a.iter().any(|(_, n)| n == name);
                assert_eq!(a, in_a(seed, name));
                seen_a[i] |= a;
                seen_b[i] |= !a;
            }
            let functions: Vec<&str> =
                plan.a.iter().filter(|(f, _)| *f).map(|(_, n)| n.as_str()).collect();
            assert!(functions.iter().all(|n| *n == "main" || *n == "f"), "{plan:?}");
        }
        // Every definition lands in both halves for some seed.
        assert!(seen_a.iter().chain(&seen_b).all(|s| *s));
        let plan = Plan { a: vec![(true, "main".to_owned()), (false, ".str".to_owned())], b: 1 };
        let args: Vec<String> = plan
            .extract_args(Path::new("p.ll"), Path::new("b.ll"), true)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["-S", "--delete", "--func=main", "--glob=.str", "-o", "b.ll", "p.ll"]);
    }
}
