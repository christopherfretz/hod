//! The per-function survey of WP-059 (`spec/dwarf.md` §6.11): what stops
//! each function's DIEs, by refusal name. With `HOD_SCOPE_SURVEY` (`.ll`
//! files or directories, `:`-separated), each module's functions are
//! compiled one by one and added to their unit's plan the way the bridge
//! adds them (`crates/hod_cg/src/emit/lines.rs`), each refusal counted
//! and the next function tried; a refusal of the unit itself (its
//! globals, imports, retained types) is counted per module, and its
//! functions are then measured without the unit's `globals:`,
//! `imports:` and `retainedTypes:`. Module `asm` is left out (the code generator's). An aid
//! only: nothing is asserted.

use std::collections::BTreeMap;

use hod_a64::A64;
use hod_cg::{Chunk, Options, Scratch as CgScratch, VarPlace, compile_function};
use hod_dwarf::{CodeSpan, FunctionInput, LocalPlace, LocalRow, ScopeRow, UnitPlan};
use hod_ir::{Body, DiNode, Module, SymbolKind};
use hod_obj::SymbolId;

/// A module and the bodies `hod_ll` reads; a body it cannot read is
/// left out.
fn parse_lenient(text: &str) -> Option<(Module, Vec<Body>)> {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default()).ok()?;
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        if let Some(body) = parsed.body {
            bodies.push(body.clone());
        }
        Ok::<(), hod_ll::LlError>(())
    })
    .ok()?;
    Some((module, bodies))
}

/// `text` with the compile units' `globals:`, `imports:` and
/// `retainedTypes:` lists removed: what the functions alone meet when the unit's own DIEs are
/// refused.
fn without_unit_lists(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if line.contains("DICompileUnit(") {
            let mut l = line.to_owned();
            for field in [", globals: !", ", imports: !", ", retainedTypes: !"] {
                if let Some(at) = l.find(field) {
                    let tail = &l[at + field.len()..];
                    let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
                    l = format!("{}{}", &l[..at], &tail[digits..]);
                }
            }
            out.push_str(&l);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// What stopped a module, per function.
#[derive(Default, Debug)]
struct Survey {
    modules: usize,
    /// Modules whose `module asm` was left out.
    module_asm: usize,
    functions: usize,
    written: usize,
    /// Refusal name to the number of functions it stopped.
    by_function: BTreeMap<String, usize>,
    /// Refusal name to the number of units it stopped as a whole.
    by_unit: BTreeMap<String, usize>,
}

fn name_of(e: &hod_dwarf::Error) -> String {
    match e {
        hod_dwarf::Error::Unsupported { what, .. } if what.starts_with("debug:") => (*what).into(),
        hod_dwarf::Error::Unsupported { what, .. } => format!("debug:die-unsupported ({what})"),
        e => format!("error: {e}"),
    }
}

fn survey_module(text: &str, s: &mut Survey) {
    let owned: String;
    let text = if text.contains("\nmodule asm ") {
        s.module_asm += 1;
        owned = text.lines().filter(|l| !l.starts_with("module asm ")).fold(
            String::with_capacity(text.len()),
            |mut acc, l| {
                acc.push_str(l);
                acc.push('\n');
                acc
            },
        );
        owned.as_str()
    } else {
        text
    };
    let Some((module, bodies)) = parse_lenient(text) else { return };
    let Some(&unit) = module.debug.compile_units().first() else { return };
    s.modules += 1;
    match UnitPlan::for_functions(&module, unit) {
        Ok(mut plan) => {
            add_functions(&module, &bodies, &mut plan, s);
            if let Err(e) = plan.seal() {
                *s.by_unit.entry(format!("seal: {}", name_of(&e))).or_default() += 1;
            }
        }
        Err(e) => {
            *s.by_unit.entry(name_of(&e)).or_default() += 1;
            let Some((module, bodies)) = parse_lenient(&without_unit_lists(text)) else { return };
            let Some(&unit) = module.debug.compile_units().first() else { return };
            match UnitPlan::for_functions(&module, unit) {
                Ok(mut plan) => {
                    add_functions(&module, &bodies, &mut plan, s);
                    if let Err(e) = plan.seal() {
                        *s.by_unit.entry(format!("seal: {}", name_of(&e))).or_default() += 1;
                    }
                }
                Err(e) => {
                    *s.by_unit
                        .entry(format!("{} (without the lists)", name_of(&e)))
                        .or_default() += 1;
                }
            }
        }
    }
}

fn add_functions(module: &Module, bodies: &[Body], plan: &mut UnitPlan<'_>, s: &mut Survey) {
    let options = Options::default();
    let mut scratch = CgScratch::new();
    for body in bodies {
        let SymbolKind::Function { subprogram: Some(sp), .. } = module.symbol(body.symbol()).kind()
        else {
            continue;
        };
        let DiNode::Subprogram(node) = module.debug.node(sp) else { continue };
        if node.unit != Some(plan.unit()) {
            continue;
        }
        s.functions += 1;
        let mut chunk = Chunk::new();
        if let Err(e) = compile_function(&A64, module, body, &options, &mut scratch, &mut chunk) {
            *s.by_function.entry(format!("codegen: {e}")).or_default() += 1;
            continue;
        }
        let rec = &chunk.funcs[0];
        let scopes: Vec<ScopeRow> = chunk.lines[rec.lines.start as usize..rec.lines.end as usize]
            .iter()
            .map(|r| ScopeRow { offset: r.offset, loc: r.loc })
            .collect();
        let vars: Vec<LocalRow> = chunk.vars[rec.vars.start as usize..rec.vars.end as usize]
            .iter()
            .map(|v| LocalRow {
                node: v.var,
                loc: v.loc,
                expr: v.expr,
                place: match v.place {
                    VarPlace::Frame(o) => LocalPlace::Frame(o),
                    VarPlace::FrameIndirect(o) => LocalPlace::FrameIndirect(o),
                    VarPlace::Label(o) => LocalPlace::Label(o),
                    VarPlace::Coro(place) => LocalPlace::Coro(place),
                    VarPlace::Refused(name) => LocalPlace::Refused(name),
                    // WP-162: the bridge's mapping (`hod_cg::dbg_value::local_place`).
                    VarPlace::Value(r) => {
                        let at = match r.at {
                            hod_cg::ValueAt::Frame(offset, size) => {
                                hod_dwarf::ValueLoc::Frame { offset, size }
                            }
                            hod_cg::ValueAt::Const { bits, width } => {
                                hod_dwarf::ValueLoc::Const { bits, width }
                            }
                            hod_cg::ValueAt::None => hod_dwarf::ValueLoc::None,
                            hod_cg::ValueAt::Home(..) => {
                                return LocalRow {
                                    place: LocalPlace::Refused("debug:frame-place"),
                                    node: v.var,
                                    loc: v.loc,
                                    expr: v.expr,
                                };
                            }
                        };
                        LocalPlace::Value { start: r.start, end: r.end, at }
                    }
                    VarPlace::Slot(_) | VarPlace::Indirect(_) => {
                        LocalPlace::Refused("debug:frame-place")
                    }
                },
            })
            .collect();
        let size = u64::from(rec.code.end - rec.code.start);
        if size == 0 {
            continue;
        }
        let input = FunctionInput {
            subprogram: sp,
            code: CodeSpan { symbol: SymbolId::from_index(1), addend: 0, size },
            frame_base: rec.frame.frame_base,
            scopes: &scopes,
            vars: &vars,
            calls: &[],
        };
        match plan.add_function(&input) {
            Ok(()) => s.written += 1,
            Err(e) => *s.by_function.entry(name_of(&e)).or_default() += 1,
        }
    }
}

/// The survey (an aid; prints what stopped what).
#[test]
#[ignore = "an aid: set HOD_SCOPE_SURVEY"]
fn survey() {
    let Some(paths) = std::env::var_os("HOD_SCOPE_SURVEY") else { return };
    let mut files = Vec::new();
    for root in std::env::split_paths(&paths) {
        let mut stack = vec![root];
        while let Some(p) = stack.pop() {
            if p.is_dir() {
                for e in std::fs::read_dir(&p).unwrap() {
                    stack.push(e.unwrap().path());
                }
            } else if p.extension().is_some_and(|x| x == "ll") {
                files.push(p);
            }
        }
    }
    files.sort();
    let mut s = Survey::default();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        if !text.contains("!llvm.dbg.cu") || text.contains("!\"Dwarf Version\", i32 4") {
            continue;
        }
        let before = (s.functions, s.written);
        survey_module(&text, &mut s);
        if std::env::var_os("HOD_SCOPE_SURVEY_EACH").is_some() {
            eprintln!(
                "{}: {} functions, {} written",
                path.display(),
                s.functions - before.0,
                s.written - before.1
            );
        }
    }
    eprintln!(
        "survey: {} modules ({} without their module asm), {} functions, {} written\n\
         by function: {:#?}\nby unit: {:#?}",
        s.modules, s.module_asm, s.functions, s.written, s.by_function, s.by_unit
    );
}
