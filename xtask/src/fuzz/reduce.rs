//! Reducing a finding by cutting the module's text
//! (`spec/landed/harness/WP-154.md` "Reduction"). Each pass offers edits
//! that keep the module valid IR, tried in chunks that halve, kept while
//! the program still shows the finding and a probe budget lasts:
//!
//! - a function body becomes a `ret` of zero (`stubs`);
//! - a function, global or declaration nothing names is deleted;
//! - a store to a global, and a call that returns nothing, is dropped;
//! - a conditional branch goes to its second target only, and the blocks
//!   no path reaches any more are deleted;
//! - a store to a local nothing reads, and an instruction whose value
//!   nothing uses, is dropped.
//!
//! Only edits that keep the program defined are offered (a stub returns a
//! value of its type; a global keeps its initial value; no local that is
//! read loses its store), and the witness, `llc -O2`, is asked about the
//! whole program: whether it agrees with the oracle.
//!
//! A `--mix` finding is reduced as a whole module that is split again by
//! every probe: the halves are a function of the names
//! ([`super::split::in_a`]), so what is left of each half stays in it, and
//! the probe builds the half hod built with hod and the other with `llc`.
//! A `--debug` finding is reduced while hod's object still fails the
//! verifier, or the line comparison still has an unclassified pair.

use std::fs;
use std::path::Path;

use super::case::{Bench, Kind, Side, Verdict, compare, ended};
use crate::irdiff::process::{End, Limits, Output};

/// What became of a reduction.
#[derive(Clone, Debug)]
pub struct Reduced {
    /// The reduced module.
    pub text: String,
    /// Probes run.
    pub probes: usize,
    /// Lines before and after.
    pub before: usize,
    pub after: usize,
    /// The oracle's program's output on the reduced module (for its seed
    /// test's `stdout` directives), when it ran.
    pub oracle: Option<Output>,
}

/// What a probe must show: the finding's kind, how the original oracle's
/// program ended, which object hod built, and the seed (for the split).
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub kind: Kind,
    pub oracle_end: Option<End>,
    pub side: Side,
    pub seed: u64,
}

/// A defined function's lines: the `define` line, the closing `}`, its
/// name and return type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub start: usize,
    pub end: usize,
    pub name: String,
    pub ret: String,
}

/// The functions defined in `lines`.
pub fn functions(lines: &[String]) -> Vec<Function> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = &lines[at];
        if let Some(header) = line.strip_prefix("define ")
            && let Some((name, ret)) = name_and_return(header)
            && let Some(close) = (at + 1..lines.len()).find(|&i| lines[i] == "}")
        {
            found.push(Function { start: at, end: close, name, ret });
            at = close + 1;
        } else {
            at += 1;
        }
    }
    found
}

/// From what follows `define `: the function's name (with `@`) and its
/// return type: the word before the name, or the braced literal struct
/// type that ends there.
fn name_and_return(header: &str) -> Option<(String, String)> {
    let at = header.find(" @")?;
    let from_name = &header[at + 1..];
    let len = from_name.find('(')?;
    let name = from_name[..len].to_owned();
    let before = header[..at].trim_end();
    // A literal struct type, packed (`<{ ... }>`) or not, else one word.
    let braced = before.strip_suffix('>').filter(|b| b.ends_with('}')).unwrap_or(before);
    let ret = if braced.ends_with('}') {
        let mut depth = 0usize;
        let (open, _) = braced.char_indices().rev().find(|&(_, c)| {
            match c {
                '}' => depth += 1,
                '{' => depth = depth.saturating_sub(1),
                _ => {}
            }
            depth == 0
        })?;
        let open = if braced.len() < before.len() { open.checked_sub(1)? } else { open };
        before[open..].to_owned()
    } else {
        before.rsplit(' ').next()?.to_owned()
    };
    Some((name, ret))
}

/// The one-line body of a stub of a function returning `ret`.
fn stub_body(ret: &str) -> String {
    if ret == "void" { "  ret void".to_owned() } else { format!("  ret {ret} zeroinitializer") }
}

/// An edit of the current lines: each index gets a new line, or none.
type Edit = Vec<(usize, Option<String>)>;

/// Whether a call or a definition writes memory its caller reads without
/// having written it: an `sret` result, or an intrinsic that fills memory
/// (`memcpy` of a struct into a local, `va_start`). Dropping or stubbing
/// it would make the program read uninitialized memory.
fn initializes(line: &str) -> bool {
    line.contains("sret(")
        || ["@llvm.memcpy", "@llvm.memmove", "@llvm.memset", "@llvm.va_start", "@llvm.va_copy"]
            .iter()
            .any(|name| line.contains(name))
}

/// The edits a pass tries.
fn stubs(lines: &[String]) -> Vec<Edit> {
    functions(lines)
        .into_iter()
        .filter(|f| f.name != "@main" && f.end > f.start + 3 && !initializes(&lines[f.start]))
        .map(|f| {
            let mut edit: Edit = vec![(f.start + 1, Some("entry:".to_owned()))];
            edit.push((f.start + 2, Some(stub_body(&f.ret))));
            edit.extend((f.start + 3..f.end).map(|i| (i, None)));
            edit
        })
        .collect()
}

fn unreferenced(lines: &[String]) -> Vec<Edit> {
    let all = functions(lines);
    all.iter()
        .filter(|f| f.name != "@main")
        .filter(|f| {
            !lines
                .iter()
                .enumerate()
                .any(|(i, line)| !(f.start..=f.end).contains(&i) && names(line, &f.name))
        })
        .map(|f| (f.start..=f.end).map(|i| (i, None)).collect())
        .collect()
}

fn droppable(lines: &[String]) -> Vec<Edit> {
    let mut edits = Vec::new();
    for f in functions(lines) {
        for (i, line) in lines.iter().enumerate().take(f.end).skip(f.start + 1) {
            let line = line.trim_start();
            let store_to_global = line.starts_with("store ") && line.contains(", ptr @");
            let call = (line.starts_with("call void ") || line.starts_with("tail call void "))
                && !initializes(line);
            if store_to_global || call {
                edits.push(vec![(i, None)]);
            }
        }
    }
    edits
}

/// Whether `line` names the value `name` (`%12`, `%x`): the name not
/// followed by a character of a longer name.
fn names(line: &str, name: &str) -> bool {
    line.match_indices(name).any(|(at, _)| {
        let next = line[at + name.len()..].bytes().next();
        !next.is_some_and(|b| b.is_ascii_alphanumeric() || b"._$-".contains(&b))
    })
}

/// Instructions whose value nothing uses (LLVM's reader, and hod's,
/// accept the gaps this leaves in the numbering of unnamed values). A
/// `phi` stays: its block may be the target of edits that remove its
/// users only in part, and it is cheap.
fn unused_values(lines: &[String]) -> Vec<Edit> {
    let mut edits = Vec::new();
    for f in functions(lines) {
        let body = &lines[f.start + 1..f.end];
        for (k, line) in body.iter().enumerate() {
            let Some((name, rest)) = line.trim_start().split_once(" = ") else { continue };
            if !name.starts_with('%') || rest.starts_with("phi ") {
                continue;
            }
            let used = body.iter().enumerate().any(|(j, other)| j != k && names(other, name));
            if !used {
                edits.push(vec![(f.start + 1 + k, None)]);
            }
        }
    }
    edits
}

/// Global variables, constants and declared functions nothing refers to.
fn unreferenced_globals(lines: &[String]) -> Vec<Edit> {
    let mut edits = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let name = match line.split_once(" = ") {
            Some((name, _)) if name.starts_with('@') && !name.contains(' ') => name,
            _ if line.starts_with("declare ") => match name_and_return(&line["declare ".len()..]) {
                Some((name, _)) => {
                    let name: &str = &name;
                    if !lines.iter().enumerate().any(|(j, other)| j != i && names(other, name)) {
                        edits.push(vec![(i, None)]);
                    }
                    continue;
                }
                None => continue,
            },
            _ => continue,
        };
        if !lines.iter().enumerate().any(|(j, other)| j != i && names(other, name)) {
            edits.push(vec![(i, None)]);
        }
    }
    edits
}

/// Stores to a local nothing reads: every other line that names the
/// pointer is its definition or another store to it.
fn dead_stores(lines: &[String]) -> Vec<Edit> {
    let mut edits = Vec::new();
    for f in functions(lines) {
        let body = &lines[f.start + 1..f.end];
        for (k, line) in body.iter().enumerate() {
            let Some(pointer) = stored_to(line) else { continue };
            let read = body.iter().any(|other| {
                names(other, &pointer)
                    && !other.trim_start().starts_with(&format!("{pointer} = alloca "))
                    && stored_to(other).as_ref() != Some(&pointer)
            });
            if !read {
                edits.push(vec![(f.start + 1 + k, None)]);
            }
        }
    }
    edits
}

/// The pointer a `store` line stores to (`%6`), if it is a value.
fn stored_to(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("store ")?;
    let (_, pointer) = rest.rsplit_once(", ptr %")?;
    Some(format!("%{}", pointer.split([',', ' ']).next().unwrap_or("")))
}

/// Conditional branches made unconditional, to their second target (the
/// exit of a loop clang writes at `-O0`). A `phi` that this leaves with an
/// entry for a block that is no longer a predecessor makes the module
/// invalid, and the probe fails: such an edit is not kept.
fn branch_cuts(lines: &[String]) -> Vec<Edit> {
    let mut edits = Vec::new();
    for f in functions(lines) {
        for (i, line) in lines.iter().enumerate().take(f.end).skip(f.start + 1) {
            if let Some(rest) = line.trim_start().strip_prefix("br i1 ")
                && let Some((_, second)) = rest.rsplit_once(", label ")
            {
                let target = second.split([',', ' ']).next().unwrap_or(second);
                edits.push(vec![(i, Some(format!("  br label {target}")))]);
            }
        }
    }
    edits
}

/// The label a line starts a block with (`12:`, `for.body:`), as a value
/// name (`%12`).
fn block_label(line: &str) -> Option<String> {
    let (label, _) = line.split_once(':')?;
    let ok = !label.is_empty()
        && label.bytes().all(|b| b.is_ascii_alphanumeric() || b"._$-".contains(&b));
    ok.then(|| format!("%{label}"))
}

/// For each function with blocks that no path from its entry reaches, one
/// edit deleting them all.
fn unreachable_blocks(lines: &[String]) -> Vec<Edit> {
    let mut edits = Vec::new();
    for f in functions(lines) {
        // Blocks: (label, first line, end line); the entry block may have
        // no label.
        let mut blocks: Vec<(Option<String>, usize, usize)> = vec![(None, f.start + 1, f.end)];
        for (i, line) in lines.iter().enumerate().take(f.end).skip(f.start + 1) {
            if let Some(label) = block_label(line) {
                if let Some(last) = blocks.last_mut() {
                    last.2 = i;
                }
                if blocks.len() == 1 && blocks[0].1 == i {
                    blocks[0].0 = Some(label);
                } else {
                    blocks.push((Some(label), i, f.end));
                }
            }
        }
        let mut reached = vec![false; blocks.len()];
        let mut work = vec![0];
        while let Some(b) = work.pop() {
            if std::mem::replace(&mut reached[b], true) {
                continue;
            }
            for line in &lines[blocks[b].1..blocks[b].2] {
                for (to, (label, _, _)) in blocks.iter().enumerate() {
                    if let Some(label) = label
                        && !reached[to]
                        && line.contains("label ")
                        && names(line, label)
                    {
                        work.push(to);
                    }
                }
            }
        }
        let edit: Edit = blocks
            .iter()
            .zip(&reached)
            .filter(|(_, reached)| !**reached)
            .flat_map(|((_, from, to), _)| (*from..*to).map(|i| (i, None)))
            .collect();
        if !edit.is_empty() {
            edits.push(edit);
        }
    }
    edits
}

fn apply(lines: &[String], edits: &[&Edit]) -> Vec<String> {
    let mut out: Vec<Option<String>> = lines.iter().cloned().map(Some).collect();
    for edit in edits {
        for (at, line) in *edit {
            out[*at].clone_from(line);
        }
    }
    out.into_iter().flatten().collect()
}

fn text_of(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// A probe that shows the finding, with the oracle's program's output
/// when it ran.
struct Shown {
    oracle: Option<Output>,
}

/// A reduction in progress.
struct Reducer<'a> {
    bench: &'a Bench,
    dir: &'a Path,
    target: Target,
    budget: usize,
    probes: usize,
    last_oracle: Option<Output>,
}

impl Reducer<'_> {
    /// Whether `lines` still show the finding.
    fn probe(&mut self, lines: &[String]) -> bool {
        self.probes += 1;
        let ir = self.dir.join("probe.ll");
        if fs::write(&ir, text_of(lines)).is_err() {
            return false;
        }
        match self.shows(&ir) {
            Some(Shown { oracle }) => {
                self.last_oracle = oracle;
                true
            }
            None => false,
        }
    }

    /// `Some` when the module at `ir` shows the finding.
    fn shows(&self, ir: &Path) -> Option<Shown> {
        match (self.target.side, self.target.kind) {
            (Side::HodA | Side::HodB, _) => self.shows_mixed(ir),
            (Side::Whole, Kind::DebugVerify | Kind::DebugLines) => self.shows_debug(ir),
            (Side::Whole, _) => self.shows_whole(ir),
        }
    }

    /// For a `--debug` finding: hod's object still fails the verifier
    /// (and `llc`'s does not), or an unclassified pair remains.
    fn shows_debug(&self, ir: &Path) -> Option<Shown> {
        let bench = self.bench;
        let mut log = Vec::new();
        let at = |name: &str| self.dir.join(name);
        let (oracle_o, hod_o) = (at("probe-oracle.o"), at("probe-hod.o"));
        bench.build_with_llc(&mut log, ir, &oracle_o, false).ok()?;
        bench.build_with_hod(&mut log, ir, &hod_o, &at("probe.tsv")).ok()?.ok()?;
        if self.target.kind == Kind::DebugVerify {
            let verdict = bench.verify(&mut log, &hod_o, &oracle_o);
            let shows = matches!(verdict, Err(Verdict::Finding(f)) if f.kind == Kind::DebugVerify);
            return shows.then_some(Shown { oracle: None });
        }
        let (oracle_x, hod_x) = (at("probe-oracle"), at("probe-hod"));
        bench.link(&mut log, &oracle_o, &oracle_x).ok()?;
        bench.link(&mut log, &hod_o, &hod_x).ok()?;
        let report = bench.compare_lines(ir, &hod_x, &oracle_x).ok()?;
        (report.unclassified() > 0).then_some(Shown { oracle: None })
    }

    /// For a `--mix` finding: the module split again, the half of the
    /// finding built by hod and the other by `llc`.
    fn shows_mixed(&self, ir: &Path) -> Option<Shown> {
        let bench = self.bench;
        let mut log = Vec::new();
        let at = |name: &str| self.dir.join(name);
        let (oracle_o, oracle_x) = (at("probe-oracle.o"), at("probe-oracle"));
        bench.build_with_llc(&mut log, ir, &oracle_o, false).ok()?;
        bench.link(&mut log, &oracle_o, &oracle_x).ok()?;
        let limits: Limits = bench.run_limits;
        let (oracle, _) = bench.execute(&mut log, &oracle_x, limits).ok()?;
        if oracle.end == End::TimedOut || Some(oracle.end) != self.target.oracle_end {
            return None;
        }
        let halves = bench.split(&mut log, self.target.seed, ir, self.dir, "probe-").ok()??;
        let (hod_half, llc_half) = match self.target.side {
            Side::HodB => (&halves.b, &halves.a),
            _ => (&halves.a, &halves.b),
        };
        let (hod_o, llc_o) = (at("probe-hod.o"), at("probe-half-llc.o"));
        let built = bench.build_with_hod(&mut log, hod_half, &hod_o, &at("probe.tsv"));
        match (self.target.kind, built) {
            (Kind::HodFailed, Err(_)) => return Some(Shown { oracle: None }),
            (kind, Ok(Ok(()))) if kind != Kind::HodFailed => {}
            _ => return None,
        }
        bench.build_with_llc(&mut log, llc_half, &llc_o, false).ok()?;
        let objects: [&Path; 2] =
            if self.target.side == Side::HodB { [&llc_o, &hod_o] } else { [&hod_o, &llc_o] };
        let program = at("probe-hod");
        let linked = bench.link_all(&mut log, &objects, &program);
        if self.target.kind == Kind::HodLink {
            return linked.is_err().then_some(Shown { oracle: None });
        }
        linked.ok()?;
        let (mixed, _) = bench.execute(&mut log, &program, limits).ok()?;
        let shows = match self.target.kind {
            Kind::HodHang => mixed.end == End::TimedOut,
            _ => {
                mixed.end != End::TimedOut
                    && matches!(compare(&oracle, &mixed), Verdict::Finding(_))
            }
        };
        shows.then_some(Shown { oracle: Some(oracle) })
    }

    /// For a finding on the whole module.
    fn shows_whole(&self, ir: &Path) -> Option<Shown> {
        let bench = self.bench;
        let mut log = Vec::new();
        let at = |name: &str| self.dir.join(name);
        bench.build_with_llc(&mut log, ir, &at("probe-oracle.o"), false).ok()?;
        let built = bench.build_with_hod(&mut log, ir, &at("probe-hod.o"), &at("probe.tsv"));
        match (self.target.kind, built) {
            (Kind::HodFailed, Err(_)) => return Some(Shown { oracle: None }),
            (_, Ok(Ok(()))) if self.target.kind != Kind::HodFailed => {}
            _ => return None,
        }
        bench.link(&mut log, &at("probe-oracle.o"), &at("probe-oracle")).ok()?;
        let linked = bench.link(&mut log, &at("probe-hod.o"), &at("probe-hod"));
        if self.target.kind == Kind::HodLink {
            return linked.is_err().then_some(Shown { oracle: None });
        }
        linked.ok()?;
        let limits: Limits = bench.run_limits;
        let (oracle, _) = bench.execute(&mut log, &at("probe-oracle"), limits).ok()?;
        if oracle.end == End::TimedOut || Some(oracle.end) != self.target.oracle_end {
            return None;
        }
        let (hod, _) = bench.execute(&mut log, &at("probe-hod"), limits).ok()?;
        let shows = match self.target.kind {
            Kind::HodHang => hod.end == End::TimedOut,
            _ => hod.end != End::TimedOut && matches!(compare(&oracle, &hod), Verdict::Finding(_)),
        };
        shows.then_some(Shown { oracle: Some(oracle) })
    }

    /// One pass: tries the edits in chunks that halve, keeping each chunk
    /// that still shows the finding.
    fn pass(&mut self, lines: Vec<String>, edits: Vec<Edit>) -> Vec<String> {
        let mut lines = lines;
        let mut pending = edits;
        let mut size = pending.len();
        // Indices refer to `base`; accepted edits are kept and applied
        // together, so that the indices of the others stay valid.
        let base = lines.clone();
        let mut accepted: Vec<Edit> = Vec::new();
        while size >= 1 && !pending.is_empty() {
            let mut at = 0;
            while at < pending.len() {
                if self.probes >= self.budget {
                    return lines;
                }
                let end = (at + size).min(pending.len());
                let trial: Vec<&Edit> = accepted.iter().chain(&pending[at..end]).collect();
                let candidate = apply(&base, &trial);
                if self.probe(&candidate) {
                    lines = candidate;
                    accepted.extend(pending.drain(at..end));
                } else {
                    at = end;
                }
            }
            if size == 1 {
                break;
            }
            size = size.div_ceil(2);
        }
        lines
    }
}

/// Reduces `text`, which shows `target`, with at most `budget` probes in
/// `dir`.
pub fn reduce(bench: &Bench, dir: &Path, text: &str, target: Target, budget: usize) -> Reduced {
    let original: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut reducer = Reducer { bench, dir, target, budget, probes: 0, last_oracle: None };
    let mut lines = original.clone();
    // Each round's edits make more candidates for the next.
    for _round in 0..6 {
        let before = lines.len();
        for pass in [
            stubs,
            unreferenced,
            droppable,
            branch_cuts,
            unreachable_blocks,
            dead_stores,
            unused_values,
            unreferenced,
            unreferenced_globals,
        ] {
            let edits = pass(&lines);
            lines = reducer.pass(lines, edits);
        }
        if lines.len() == before || reducer.probes >= budget {
            break;
        }
    }
    let oracle = if lines == original { None } else { reducer.last_oracle.take() };
    for name in [
        "probe.ll",
        "probe-oracle.o",
        "probe-hod.o",
        "probe.tsv",
        "probe-oracle",
        "probe-hod",
        "probe-a.ll",
        "probe-b.ll",
        "probe-half-llc.o",
    ] {
        let _ = fs::remove_file(dir.join(name));
    }
    Reduced {
        text: text_of(&lines),
        probes: reducer.probes,
        before: original.len(),
        after: lines.len(),
        oracle,
    }
}

/// Builds `ir` with `llc -O2`, runs it, and says whom its program agrees
/// with: the oracle's program or hod's. Files go to `dir`.
pub fn witness(bench: &Bench, dir: &Path, ir: &Path, oracle: &Output, hod: &Output) -> String {
    let mut log = Vec::new();
    let (object, program) = (dir.join("witness.o"), dir.join("witness"));
    if let Err(e) = bench.build_with_llc(&mut log, ir, &object, true) {
        return format!("llc -O2 failed: {e}");
    }
    if let Err(e) = bench.link(&mut log, &object, &program) {
        return format!("the witness's link failed: {e}");
    }
    let limits = Limits { time: bench.run_limits.time * 4, ..bench.run_limits };
    let witness = match bench.execute(&mut log, &program, limits) {
        Ok((witness, _)) => witness,
        Err(e) => return format!("the witness did not run: {e}"),
    };
    let _ = fs::write(dir.join("witness.stdout"), &witness.stdout);
    let same = |other: &Output| other.end == witness.end && other.stdout == witness.stdout;
    match (same(oracle), same(hod)) {
        (true, true) => "llc -O2 agrees with both (the difference is not reproducible)".to_owned(),
        (true, false) => "llc -O2 agrees with llc -O0 (the oracle): hod is suspect".to_owned(),
        (false, true) => {
            "llc -O2 agrees with hod: the oracle or the program (undefined behaviour) is suspect"
                .to_owned()
        }
        (false, false) => format!("llc -O2 agrees with neither; it {}", ended(witness.end)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    const MODULE: &str = "\
@g = global i32 0
define internal { i32, i8 } @pair(i32 %a) #0 {
entry:
  %x = add i32 %a, 1
  store i32 %x, ptr @g, align 4
  ret { i32, i8 } zeroinitializer
}
define dso_local noundef signext i8 @small() {
entry:
  call void @pair_user()
  ret i8 1
}
define internal void @pair_user() {
entry:
  %p = call { i32, i8 } @pair(i32 1)
  ret void
}
define dso_local i32 @main() {
entry:
  store i32 1, ptr %local, align 4
  call void @pair_user()
  ret i32 0
}";

    // spec/landed/harness/WP-154.md "Reduction": the functions of a module.
    #[test]
    fn functions_and_their_return_types() {
        let found = functions(&lines(MODULE));
        let names: Vec<(&str, &str)> =
            found.iter().map(|f| (f.name.as_str(), f.ret.as_str())).collect();
        assert_eq!(
            names,
            [("@pair", "{ i32, i8 }"), ("@small", "i8"), ("@pair_user", "void"), ("@main", "i32")]
        );
        assert_eq!((found[0].start, found[0].end), (1, 6));
        assert_eq!(
            name_and_return("internal <{ i8, i16 }> @packed(i32 %a) {"),
            Some(("@packed".to_owned(), "<{ i8, i16 }>".to_owned()))
        );
    }

    // "Reduction": what each pass tries.
    #[test]
    fn edits_of_each_pass() {
        let module = lines(MODULE);
        // Bodies of more than a label and a `ret`, not main.
        let stubbed = stubs(&module);
        assert_eq!(stubbed.len(), 3, "{stubbed:?}");
        let text = apply(&module, &[&stubbed[0]]);
        assert_eq!(
            text[1..5],
            lines(
                "define internal { i32, i8 } @pair(i32 %a) #0 {\nentry:\n  ret { i32, i8 } zeroinitializer\n}"
            )
        );
        // @small is referred to by nobody; @pair_user is called.
        let unused = unreferenced(&module);
        assert_eq!(unused.len(), 1);
        let text = apply(&module, &[&unused[0]]);
        assert!(!text.iter().any(|line| line.contains("@small")));
        // Stores to globals and calls of void functions; not the store to
        // a local, not a call that defines a value, not one that fills
        // memory (an sret result, a memcpy).
        let mut filled = module.clone();
        filled[9] =
            "  call void @llvm.memcpy.p0.p0.i64(ptr %d, ptr @g, i64 4, i1 false)".to_owned();
        assert!(!droppable(&filled).iter().any(|edit| edit[0].0 == 9));
        filled[1] = "define internal void @pair(ptr sret({ i32, i8 }) %r, i32 %a) #0 {".to_owned();
        assert_eq!(stubs(&filled).len(), 2);
        let dropped: Vec<usize> = droppable(&module).iter().map(|edit| edit[0].0).collect();
        assert_eq!(dropped, [4, 9, 20]);
        // `%p` is used by nobody; `%x` by the store; `%local` is not
        // defined here.
        let unused: Vec<usize> = unused_values(&module).iter().map(|edit| edit[0].0).collect();
        assert_eq!(unused, [14]);
        assert!(names("  ret i32 %1", "%1") && !names("  ret i32 %12", "%1"));
        // `@g` is stored to; nothing names `@h`.
        let mut with_h = module.clone();
        with_h.insert(1, "@h = internal constant i8 1, align 1".to_owned());
        with_h.push("declare i32 @printf(ptr noundef, ...) #2".to_owned());
        with_h.push("declare void @used(ptr)".to_owned());
        with_h[20].push_str(" ; @used");
        let globals: Vec<usize> =
            unreferenced_globals(&with_h).iter().map(|edit| edit[0].0).collect();
        assert_eq!(globals, [1, 24]);
        // main's store to %local: nothing reads it.
        let stores: Vec<usize> = dead_stores(&module).iter().map(|edit| edit[0].0).collect();
        assert_eq!(stores, [19]);
    }

    // "Reduction": branches and the blocks they no longer reach.
    #[test]
    fn branches_and_unreachable_blocks() {
        let module = lines(
            "define i32 @main() {\n  %1 = icmp eq i32 1, 2\n  br i1 %1, label %2, label %4\n\
             2:\n  %3 = add i32 1, 1\n  br label %2\n\
             4:                ; preds = %0\n  ret i32 0\n}",
        );
        let cuts = branch_cuts(&module);
        assert_eq!(cuts, [vec![(2, Some("  br label %4".to_owned()))]]);
        assert!(unreachable_blocks(&module).is_empty());
        let cut = apply(&module, &[&cuts[0]]);
        // The loop at %2 is reached by nobody now, though it branches to
        // itself.
        let gone = unreachable_blocks(&cut);
        assert_eq!(gone.len(), 1);
        let left = apply(&cut, &[&gone[0]]);
        assert_eq!(
            left,
            lines(
                "define i32 @main() {\n  %1 = icmp eq i32 1, 2\n  br label %4\n4:                ; preds = %0\n  ret i32 0\n}"
            )
        );
    }
}
