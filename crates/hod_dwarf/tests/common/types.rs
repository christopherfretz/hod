//! Shared by the type-graph tests (`spec/dwarf.md` §8.6): C++ compiled
//! to `-g` IR, the oracle input made from it, and hod's DIE sections for
//! `llc`'s object of a module read by `hod_ll`.
//!
//! **The oracle input.** hod writes no function DIEs yet (WP-057), so a
//! module with functions is compared without them, and with the types
//! the functions reached kept alive as retained types:
//!
//! 1. the type of every local variable and of every function definition,
//!    in metadata order, joins the unit's `retainedTypes`, unless it
//!    reaches something function-local (a definition, a block, a local, a
//!    label, a location: a lambda's class, a function's static);
//! 2. the unit's `globals` lose the variables that are declarations or
//!    static members' definitions, have an expression, are scoped in a
//!    class, or reach something function-local (WP-059's and WP-057's);
//!    their `!dbg` attachments go too;
//! 3. the unit's `imports` go (WP-059's), and so do its function-local
//!    `enums`;
//! 4. `private` globals become `internal`, so that `llc` gives them a
//!    symbol hod's addresses can name;
//! 5. `llvm-extract --delete --rfunc '.*'` deletes the functions' bodies.
//!
//! hod reads the result; `llc` reads it without `templateParams` (WP-059
//! writes those), so neither writes the template parameters' DIEs nor
//! the types only they reach.

use std::collections::{HashMap, VecDeque};
use std::path::Path;

use hod_dwarf::{
    DieSections, GlobalAddr, LineSections, UnitInput, UnitPlan, write_debug_info, write_debug_line,
};
use hod_ir::Module;
use hod_obj::{Arch, Object, SymbolId};
use hod_testutil::{Scratch, run};

use super::Elf;
use super::dies::stand_in;

/// The flags every C++ source of the type tests is compiled with.
pub const CXX_FLAGS: [&str; 7] =
    ["-std=c++20", "-g", "-O0", "-fstandalone-debug", "-S", "-emit-llvm", "-w"];

/// `-g` IR of `source` by `compiler`, for `target` (or the host).
pub fn cxx(
    compiler: &Path,
    scratch: &Scratch,
    name: &str,
    source: &Path,
    target: Option<&str>,
) -> String {
    let out = scratch.join(format!("{name}.ll"));
    let mut args: Vec<std::ffi::OsString> = CXX_FLAGS.iter().map(Into::into).collect();
    if let Some(t) = target {
        args.push(format!("--target={t}").into());
    }
    args.push(source.into());
    args.push("-o".into());
    args.push(out.clone().into());
    let o = run(compiler, &args);
    assert!(o.success(), "{} {name}: {}", compiler.display(), o.stderr_text());
    std::fs::read_to_string(out).unwrap()
}

fn number(s: &str) -> Option<u32> {
    s.strip_prefix('!').and_then(|n| n.parse().ok())
}

/// The `!N` references of a metadata line's right-hand side.
fn refs(rhs: &str) -> Vec<u32> {
    let b = rhs.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut quoted = false;
    while i < b.len() {
        match b[i] {
            b'\\' if quoted => i += 1,
            b'"' => quoted = !quoted,
            b'!' if !quoted => {
                let digits = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
                if digits > 0 {
                    out.push(rhs[i + 1..i + 1 + digits].parse().unwrap());
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The value of field `field` (`field: !N`) of a node's text.
fn field(text: &str, field: &str) -> Option<u32> {
    let at = text.find(&format!("{field}: !"))?;
    let rest = &text[at + field.len() + 2..];
    let end = rest[1..].find(|c: char| !c.is_ascii_digit()).map_or(rest.len(), |e| e + 1);
    number(&rest[..end])
}

fn tuple(text: &str) -> Vec<u32> {
    let Some(body) = text.strip_prefix("!{") else { return Vec::new() };
    body.trim_end_matches('}').split(',').filter_map(|e| number(e.trim())).collect()
}

/// `text` without `, templateParams: !N`.
pub fn strip_template_params(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(", templateParams: !") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 19..];
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        rest = &tail[digits..];
    }
    out.push_str(rest);
    out
}

/// Steps 1 to 4 of the oracle input (the module docs), on IR text.
pub fn oracle_input(ir: &str) -> String {
    let mut nodes: HashMap<u32, &str> = HashMap::new();
    let mut order = Vec::new();
    let mut max = 0;
    for line in ir.lines() {
        if let Some((lhs, rhs)) = line.split_once(" = ")
            && let Some(n) = number(lhs)
        {
            nodes.insert(n, rhs.strip_prefix("distinct ").unwrap_or(rhs));
            order.push(n);
            max = max.max(n);
        }
    }
    let kind = |n: u32| nodes.get(&n).map_or("", |t| t.split('(').next().unwrap_or(""));
    let local = |n: u32| {
        let t = nodes.get(&n).copied().unwrap_or("");
        match kind(n) {
            "!DISubprogram" => t.contains("DISPFlagDefinition"),
            "!DILexicalBlock"
            | "!DILexicalBlockFile"
            | "!DILocalVariable"
            | "!DILabel"
            | "!DILocation" => true,
            _ => false,
        }
    };
    // Reversed edges, template parameters left out, nothing out of a
    // unit or a file.
    let mut users: HashMap<u32, Vec<u32>> = HashMap::new();
    for &n in &order {
        if matches!(kind(n), "!DICompileUnit" | "!DIFile") {
            continue;
        }
        for r in refs(&strip_template_params(nodes[&n])) {
            users.entry(r).or_default().push(n);
        }
    }
    let mut bad: std::collections::HashSet<u32> =
        order.iter().copied().filter(|&n| local(n)).collect();
    let mut queue: VecDeque<u32> = bad.iter().copied().collect();
    while let Some(n) = queue.pop_front() {
        for &u in users.get(&n).map_or(&[][..], Vec::as_slice) {
            if bad.insert(u) {
                queue.push_back(u);
            }
        }
    }

    let cu = *order.iter().find(|&&n| kind(n) == "!DICompileUnit").expect("a unit");
    let cu_text = nodes[&cu];
    let list = |f: &str| field(cu_text, f).map(|t| tuple(nodes[&t])).unwrap_or_default();
    let mut retained = list("retainedTypes");
    for &n in &order {
        let is_root = kind(n) == "!DILocalVariable" || (local(n) && kind(n) == "!DISubprogram");
        if let Some(t) = field(nodes[&n], "type").filter(|_| is_root)
            && !bad.contains(&t)
            && !retained.contains(&t)
        {
            retained.push(t);
        }
    }
    let enums: Vec<u32> = list("enums").into_iter().filter(|e| !bad.contains(e)).collect();
    let mut dropped = Vec::new();
    let globals: Vec<u32> = list("globals")
        .into_iter()
        .filter(|&g| {
            let text = nodes[&g];
            let var = field(text, "var").unwrap();
            let v = nodes[&var];
            let keep = text.contains("expr: !DIExpression())")
                && !v.contains("declaration: ")
                && !bad.contains(&var)
                && field(v, "scope").is_none_or(|s| kind(s) != "!DICompositeType");
            if !keep {
                dropped.push(g);
            }
            keep
        })
        .collect();

    let join = |v: &[u32]| v.iter().map(|n| format!("!{n}")).collect::<Vec<_>>().join(", ");
    let (rt, gl, en) = (max + 1, max + 2, max + 3);
    let mut new_cu = cu_text.to_string();
    for f in ["retainedTypes", "globals", "imports", "enums"] {
        if let Some(t) = field(&new_cu, f) {
            new_cu = new_cu.replace(&format!(", {f}: !{t}"), "");
        }
    }
    let close = new_cu.rfind(')').unwrap();
    new_cu.insert_str(close, &format!(", enums: !{en}, retainedTypes: !{rt}, globals: !{gl}"));

    let mut out = String::with_capacity(ir.len() + 4096);
    for line in ir.lines() {
        if let Some((lhs, _)) = line.split_once(" = ")
            && number(lhs) == Some(cu)
        {
            out.push_str(&format!("!{cu} = distinct {new_cu}\n"));
            continue;
        }
        let mut l = line.to_string();
        if l.starts_with('@') {
            for &g in &dropped {
                l = l.replace(&format!(", !dbg !{g}"), "");
            }
            l = l.replacen(" = private ", " = internal ", 1);
        }
        out.push_str(&l);
        out.push('\n');
    }
    out.push_str(&format!(
        "!{rt} = !{{{}}}\n!{gl} = !{{{}}}\n!{en} = !{{{}}}\n",
        join(&retained),
        join(&globals),
        join(&enums)
    ));
    out
}

/// Step 5: the module without its functions' bodies.
pub fn extract(llvm_extract: &Path, scratch: &Scratch, name: &str, ir: &str) -> String {
    let input = scratch.write(&format!("{name}.in.ll"), ir);
    let out = scratch.join(format!("{name}.x.ll"));
    let o = run(
        llvm_extract,
        [
            "--delete".into(),
            "--rfunc".into(),
            ".*".into(),
            "-S".into(),
            input.into_os_string(),
            "-o".into(),
            out.clone().into_os_string(),
        ],
    );
    assert!(o.success(), "llvm-extract {name}: {}", o.stderr_text());
    std::fs::read_to_string(out).unwrap()
}

/// The module `hod_ll` reads from `text`.
pub fn read(text: &str) -> Module {
    hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e:?}"))
        .module
}

/// hod's debug sections for `llc`'s object of `module` (functions
/// deleted): the line table from the plan's files, the DIEs from the
/// module's tables (`UnitPlan::of_module`), each global relocated
/// against `llc`'s symbol of the same name.
pub fn module_sections(
    elf: &Elf,
    arch: Arch,
    module: &Module,
) -> Result<(Object, LineSections, DieSections, usize), hod_dwarf::Error> {
    let unit = module.debug.compile_units()[0];
    let plan = UnitPlan::of_module(module, unit)?;
    let mut obj = stand_in(elf, arch);
    let line = write_debug_line(&mut obj, &plan.files().header(), &[]).expect("line table");
    let globals: Vec<GlobalAddr> = module
        .global_exprs()
        .iter()
        .filter_map(|&(sym, expr)| {
            let name = String::from_utf8_lossy(module.symbol(sym).name()).into_owned();
            let at = elf.symbols.iter().position(|s| s.name == name)?;
            Some(GlobalAddr { expr, symbol: SymbolId::from_index(at as u32 - 1), addend: 0 })
        })
        .collect();
    let input = UnitInput { plan: &plan, line_offset: 0, globals: &globals, code: &[] };
    let dies = write_debug_info(&mut obj, line.line, &[input])?;
    Ok((obj, line, dies, plan.die_count()))
}
