//! The plan against LLVM's coroutine frame, over a directory of
//! pre-optimisation modules (`HOD_XFORM_PREOPT`, searched for `*.ll`);
//! skipped without it (`spec/xform.md` §4).
//!
//! For every coroutine `opt -passes=coro-early,cgscc(coro-split),coro-cleanup`
//! builds, the allocas and values LLVM keeps in `%F.Frame` must be in
//! hod's frame: `heap_slots` ∪ `begin_copies` ∪ the promise for allocas,
//! `heap_values` for spilled values. The only exemption is a slot hod
//! classes by rule 3 (unescaped), whose own dataflow is more precise than
//! LLVM's. And the bodies [`hod_xform::coro::plan`] refuses must be
//! exactly those of the contract-postcondition modules, by
//! `coro:frame-access-after-suspend`.
//!
//! The module's version is the `llvm19`/`llvm22` component of its path;
//! `opt` is `/usr/lib/llvm-<major>/bin/opt`.

mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_ir::{Body, InstId, InstKind, Module, SlotId, ValueKind};
use hod_xform::coro::{FRAME_ACCESS_AFTER_SUSPEND, SlotClass};

fn modules(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            modules(&path, out);
        } else if path.extension().is_some_and(|e| e == "ll") {
            out.push(path);
        }
    }
}

/// `%N` → `%vN` and `N:` → `vN:` inside function bodies, and an explicit
/// label on each entry block: `opt` then names each frame field after the
/// value it holds (`%vN.reload.addr`, `%vN.spill.addr`).
fn rename(text: &str) -> String {
    fn numbered(line: &str) -> String {
        let bytes = line.as_bytes();
        let mut out = String::with_capacity(line.len() + 8);
        let mut at = 0;
        // A label `N:` at the start of the line.
        let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
        if digits > 0 && bytes.get(digits) == Some(&b':') {
            out.push('v');
        }
        while at < bytes.len() {
            let c = bytes[at];
            out.push(c as char);
            at += 1;
            if c == b'%' && bytes.get(at).is_some_and(u8::is_ascii_digit) {
                let end = at + bytes[at..].iter().take_while(|b| b.is_ascii_digit()).count();
                let word = bytes.get(end).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
                if !word {
                    out.push('v');
                }
            }
        }
        // Only ASCII is rewritten; the rest is copied byte for byte.
        out
    }
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let mut inside = false;
    for line in text.split_inclusive('\n') {
        let start = line.starts_with("define ");
        inside |= start;
        let line = if inside && line.is_ascii() { numbered(line) } else { line.to_owned() };
        out.push_str(&line);
        if start && line.trim_end().ends_with('{') {
            let args = line
                .match_indices("%v")
                .filter(|&(at, _)| line.as_bytes().get(at + 2).is_some_and(u8::is_ascii_digit))
                .count();
            let _ = writeln!(out, "v{args}:");
        }
        if line.trim_end() == "}" {
            inside = false;
        }
    }
    out
}

/// Per function of the renamed text: the names of its entry block's
/// allocas, in order, and the line defining each `%vN`.
struct Source<'t> {
    allocas: HashMap<String, Vec<&'t str>>,
    defs: HashMap<String, HashMap<&'t str, &'t str>>,
}

fn function_name(define: &str) -> Option<String> {
    let at = define.find(" @")? + 2;
    let rest = &define[at..];
    if let Some(quoted) = rest.strip_prefix('"') {
        return Some(quoted[..quoted.find('"')?].to_owned());
    }
    let end = rest.find('(')?;
    Some(rest[..end].to_owned())
}

fn source(text: &str) -> Source<'_> {
    let mut allocas: HashMap<String, Vec<&str>> = HashMap::new();
    let mut defs: HashMap<String, HashMap<&str, &str>> = HashMap::new();
    let mut current: Option<String> = None;
    let mut entry = false;
    for line in text.lines() {
        if line.starts_with("define ") {
            current = function_name(line);
            entry = true;
            continue;
        }
        let Some(name) = &current else { continue };
        if line == "}" {
            current = None;
            continue;
        }
        let label = !line.starts_with(' ') && line.trim_end().ends_with(':');
        if label && !allocas.get(name).is_none_or(Vec::is_empty) {
            entry = false;
        }
        let trimmed = line.trim_start();
        if let Some((value, rest)) = trimmed.split_once(" = ")
            && value.starts_with("%v")
        {
            defs.entry(name.clone()).or_default().insert(value, rest);
            if entry && rest.starts_with("alloca ") {
                allocas.entry(name.clone()).or_default().push(value);
            }
        }
    }
    Source { allocas, defs }
}

/// What `opt` keeps in each frame: frame (function) name → value names.
fn frames(split: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in split.lines() {
        let line = line.trim_start();
        let Some((value, rest)) = line.split_once(" = getelementptr inbounds ") else { continue };
        let Some(ty) = rest.split(',').next() else { continue };
        let Some(function) = ty.trim_matches(|c| c == '%' || c == '"').strip_suffix(".Frame")
        else {
            continue;
        };
        let value = value.trim_start_matches('%');
        let base = value
            .split_once(".reload.addr")
            .or_else(|| value.split_once(".spill.addr"))
            .map(|(base, _)| base);
        if let Some(base) = base {
            out.entry(function.trim_matches('"').to_owned())
                .or_default()
                .insert(format!("%{base}"));
        }
    }
    out
}

/// A heap value's signature: its callee for a call, else its kind.
fn signature(module: &Module, body: &Body, value: InstId) -> String {
    let inst = body.inst(value);
    if let InstKind::Call(call) = inst.kind()
        && let ValueKind::Sym(sym) = call.callee_value().kind()
    {
        return format!("@{}", String::from_utf8_lossy(module.symbol(sym).name()));
    }
    format!("{:?}", inst.opcode())
}

/// The same signature from a defining line of the text.
fn text_signature(def: &str) -> String {
    if let Some(at) = def.find(" @") {
        let rest = &def[at + 2..];
        let end = rest.find(['(', ' ', ',']).unwrap_or(rest.len());
        return format!("@{}", rest[..end].trim_matches('"'));
    }
    def.split_whitespace().next().unwrap_or("").to_owned()
}

fn opt(major: &str, text: &str) -> String {
    let mut command = Command::new(format!("/usr/lib/llvm-{major}/bin/opt"));
    command.args(["-S", "-passes=coro-early,cgscc(coro-split),coro-cleanup", "-", "-o", "-"]);
    let outcome = hod_testutil::run_with(&mut command, text.as_bytes(), Duration::from_mins(2));
    assert!(outcome.success(), "opt-{major} failed:\n{}", outcome.stderr_text());
    outcome.stdout_text()
}

fn is_expected_refusal(path: &Path) -> bool {
    let name = path.file_name().unwrap().to_string_lossy();
    ["post-enforce", "post-observe", "post-quick-enforce"]
        .iter()
        .any(|kind| name == format!("run-coroutine-{kind}.cpp.ll"))
        // Design 0005's seed of the same hazard, if the directory has it.
        || name.starts_with("post-sync")
}

#[derive(Default)]
struct Totals {
    modules: usize,
    coroutines: usize,
    llvm_allocas: usize,
    rule3_exempt: usize,
    hod_heap: usize,
    hod_extra: usize,
    spills: usize,
    refused: BTreeMap<String, usize>,
}

#[test]
#[allow(clippy::too_many_lines, reason = "one comparison, in order")]
#[allow(clippy::cast_precision_loss, reason = "a printed measurement")]
fn frames_against_llvm() {
    let Ok(dir) = std::env::var("HOD_XFORM_PREOPT") else { return };
    let mut paths = Vec::new();
    modules(Path::new(&dir), &mut paths);
    let mut totals = Totals::default();
    let mut failures = Vec::new();
    let mut refused_modules = BTreeSet::new();
    // The largest coroutine body: (instructions, best ns of `plan`, name).
    let mut largest: (usize, f64, String) = (0, 0.0, String::new());
    for path in &paths {
        let shown = path.strip_prefix(&dir).unwrap_or(path).display().to_string();
        let major = if shown.contains("llvm19") { "19" } else { "22" };
        let text = std::fs::read_to_string(path).unwrap();
        let (module, functions) = common::parse_all(text.as_bytes());
        let renamed = rename(&text);
        let src = source(&renamed);
        let mut split: Option<BTreeMap<String, BTreeSet<String>>> = None;
        totals.modules += 1;
        for function in &functions {
            let Ok(body) = &function.body else {
                if function
                    .body
                    .as_ref()
                    .err()
                    .is_some_and(|u| u.iter().any(|c| c.contains("presplit")))
                {
                    failures.push(format!(
                        "{shown} {}: unparsed {:?}",
                        function.name,
                        function.body.as_ref().err()
                    ));
                }
                continue;
            };
            let analysis = match hod_xform::coro::analyze(&module, body) {
                Ok(None) => continue,
                Ok(Some(analysis)) => analysis,
                Err(refusal) => {
                    failures.push(format!("{shown} {}: {refusal}", function.name));
                    continue;
                }
            };
            totals.coroutines += 1;
            if body.inst_count() > largest.0 {
                let mut best = f64::INFINITY;
                for _ in 0..20 {
                    let start = std::time::Instant::now();
                    let _ = hod_xform::coro::plan(&module, body);
                    best = best.min(start.elapsed().as_secs_f64() * 1e9);
                }
                largest = (body.inst_count(), best, format!("{shown} {}", function.name));
            }
            match hod_xform::coro::plan(&module, body) {
                Ok(_) => {}
                Err(refusal) => {
                    *totals.refused.entry(refusal.construct.to_owned()).or_default() += 1;
                    refused_modules.insert(shown.clone());
                    if refusal.construct != FRAME_ACCESS_AFTER_SUSPEND || !is_expected_refusal(path)
                    {
                        failures.push(format!("{shown} {}: unexpected {refusal}", function.name));
                    }
                }
            }
            let frames = split.get_or_insert_with(|| frames(&opt(major, &renamed)));
            let Some(frame) = frames.get(&function.name) else {
                failures.push(format!("{shown} {}: no frame from opt", function.name));
                continue;
            };
            let allocas = src.allocas.get(&function.name).cloned().unwrap_or_default();
            if allocas.len() != body.slots().len() {
                failures.push(format!(
                    "{shown} {}: {} entry allocas, {} slots",
                    function.name,
                    allocas.len(),
                    body.slots().len()
                ));
                continue;
            }
            let plan = &analysis.plan;
            let in_frame = |slot: SlotId| {
                plan.heap_slots.contains(slot)
                    || plan.begin_copies.contains(&slot)
                    || plan.promise == Some(slot)
            };
            let mut llvm_slots = BTreeSet::new();
            let mut spill_sigs: Vec<String> = Vec::new();
            for name in frame {
                if let Some(k) = allocas.iter().position(|a| a == name) {
                    llvm_slots.insert(k);
                    totals.llvm_allocas += 1;
                    let slot = SlotId::new(k as u32);
                    if in_frame(slot) {
                        continue;
                    }
                    if analysis.classes[k] == SlotClass::Unescaped {
                        totals.rule3_exempt += 1;
                        continue;
                    }
                    failures.push(format!(
                        "{shown} {}: LLVM keeps {name} (slot {k}, {:?}) in the frame, hod does not",
                        function.name, analysis.classes[k]
                    ));
                } else if let Some(def) =
                    src.defs.get(&function.name).and_then(|d| d.get(name.as_str()))
                {
                    totals.spills += 1;
                    spill_sigs.push(text_signature(def));
                } else {
                    failures
                        .push(format!("{shown} {}: frame field {name} not found", function.name));
                }
            }
            let mut ours: Vec<String> =
                plan.heap_values.iter().map(|v| signature(&module, body, v)).collect();
            for sig in &spill_sigs {
                if let Some(at) = ours.iter().position(|o| o == sig) {
                    ours.swap_remove(at);
                } else {
                    failures.push(format!(
                        "{shown} {}: LLVM spills a value of {sig}, hod has no such heap value",
                        function.name
                    ));
                }
            }
            for slot in plan.heap_slots.iter() {
                totals.hod_heap += 1;
                if !llvm_slots.contains(&slot.index()) {
                    totals.hod_extra += 1;
                }
            }
        }
    }
    println!(
        "{} modules, {} coroutines; LLVM frame allocas {} (rule-3 exempt {}), spills {}; hod heap slots {} ({} not in LLVM's frame); refused {:?} in {} modules",
        totals.modules,
        totals.coroutines,
        totals.llvm_allocas,
        totals.rule3_exempt,
        totals.spills,
        totals.hod_heap,
        totals.hod_extra,
        totals.refused,
        refused_modules.len(),
    );
    println!(
        "largest coroutine body: {} ({} instructions): plan {:.0} ns, {:.1} ns/instruction",
        largest.2,
        largest.0,
        largest.1,
        largest.1 / largest.0.max(1) as f64
    );
    for module in &refused_modules {
        println!("refused: {module}");
    }
    let expected: Vec<&PathBuf> = paths.iter().filter(|p| is_expected_refusal(p)).collect();
    if refused_modules.len() != expected.len() {
        failures.push(format!(
            "{} modules refused, {} expected",
            refused_modules.len(),
            expected.len()
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
