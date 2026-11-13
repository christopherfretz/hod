//! DWARF 4 call sites (WP-150, `spec/dwarf.md` §6.21): for a subprogram
//! with `DIFlagAllCallsDescribed` in a version 4 unit, `llc` 19 and 22
//! write GNU call sites (`DW_AT_GNU_all_call_sites`,
//! `DW_TAG_GNU_call_site` with `DW_AT_abstract_origin` or
//! `DW_AT_GNU_call_site_target_clobbered`, `DW_AT_GNU_tail_call`, and
//! `DW_AT_low_pc`, the end of the call or branch instruction), on AArch64
//! and x86-64.
//!
//! - `trees_equal_llc`: the DIE trees of `tests/data/call_sites/*` by
//!   clang 19 (`llc` 19), clang 22 and kiln++ (`llc` 22; the flag added
//!   to kiln's definitions) with `-gdwarf-4` equal `llc`'s after §6.7's
//!   normalization, both objects `--verify` clean, the call sites'
//!   origins and tail flags the same in order, and every `DW_AT_low_pc`
//!   of BOTH objects the end of a call (or, for a tail call, a branch) of
//!   the origin or through the target register;
//! - a flagless module (`llc`'s version 4) gets the same call sites;
//! - `line_tables_only_matches_llc`: `-gline-tables-only` (both
//!   versions): the minimal subprogram with the flag, its call sites, and
//!   the empty DIE of a callee without a function DIE (§6.21);
//! - `thread_local_models`: in every TLS model, on both targets, at
//!   versions 4 and 5, the model's runtime calls get no call site (from
//!   hod or `llc`), the ordinary call one.
//!
//! The harness is `dwarf_call_sites.rs`'s (WP-113), with version 4's forms.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_testutil::{Scratch, run_with, tool, tools_required};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/call_sites")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Arch {
    A64,
    X64,
}

impl Arch {
    fn triple(self) -> &'static str {
        match self {
            Self::A64 => "aarch64-unknown-linux-gnu",
            Self::X64 => "x86_64-unknown-linux-gnu",
        }
    }
}

const ARCHES: [Arch; 2] = [Arch::A64, Arch::X64];

fn output(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

/// kiln++ beside this build (`.build/target-kiln`), or `HOD_KILN`.
fn kiln() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HOD_KILN").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let beside = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .ancestors()
        .nth(2)
        .map(|root| root.join("target-kiln").join("release").join("kiln++"));
    match beside {
        Some(path) if path.is_file() => Some(path),
        _ => {
            assert!(!tools_required(), "no kiln++ found; set HOD_KILN (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: no kiln++ (set HOD_KILN); skipping its part");
            None
        }
    }
}

/// A front end and the `llc` that reads its IR.
struct Frontend {
    name: &'static str,
    compiler: PathBuf,
    opt: Option<PathBuf>,
    llc: PathBuf,
}

fn frontends(cxx: bool) -> Vec<Frontend> {
    let mut out = Vec::new();
    for major in [19, 22] {
        let cc = if cxx { format!("clang++-{major}") } else { format!("clang-{major}") };
        if let (Some(compiler), Some(opt), Some(llc)) =
            (tool(&cc), tool(&format!("opt-{major}")), tool(&format!("llc-{major}")))
        {
            let name = if major == 19 { "c19" } else { "c22" };
            out.push(Frontend { name, compiler, opt: Some(opt), llc });
        }
    }
    if let (Some(compiler), Some(llc)) = (kiln(), tool("llc-22")) {
        out.push(Frontend { name: "kiln", compiler, opt: None, llc });
    }
    out
}

/// kiln's `-O0` IR never sets the flag: it is added to every definition.
fn describe_calls(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4096);
    for line in text.lines() {
        if line.contains("= distinct !DISubprogram(") && line.contains("DISPFlagDefinition") {
            if line.contains(" flags: ") {
                out.push_str(&line.replacen(" flags: ", " flags: DIFlagAllCallsDescribed | ", 1));
            } else {
                out.push_str(&line.replacen(
                    "spFlags:",
                    "flags: DIFlagAllCallsDescribed, spFlags:",
                    1,
                ));
            }
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// The IR of `source` for `arch` by `fe`, with `extra` flags.
fn ir(fe: &Frontend, scratch: &Scratch, arch: Arch, source: &str, extra: &[&str]) -> String {
    let tag = format!("{source}-{}-{arch:?}-4", fe.name);
    let mut input = data_dir().join(source);
    let kiln = fe.opt.is_none();
    if kiln && Path::new(source).extension().is_some_and(|e| e == "c") {
        let text = std::fs::read_to_string(&input).unwrap();
        input = scratch.write(&format!("{tag}.cpp"), text);
    }
    let raw = scratch.join(format!("{tag}.raw.ll"));
    let mut command = Command::new(&fe.compiler);
    command.arg(format!("--target={}", arch.triple()));
    if kiln {
        command.args(["-g", "-gdwarf-4", "-O0", "-std=c++20"]);
    } else {
        command.args(["-g", "-gdwarf-4", "-O1", "-Xclang", "-disable-llvm-passes"]);
    }
    command.args(["-S", "-emit-llvm", "-w"]).args(extra).arg(&input).arg("-o").arg(&raw);
    output(&mut command);
    let Some(opt) = &fe.opt else {
        return describe_calls(&std::fs::read_to_string(raw).unwrap());
    };
    let done = scratch.join(format!("{tag}.ll"));
    output(Command::new(opt).args(["-passes=always-inline", "-S"]).arg(&raw).arg("-o").arg(&done));
    std::fs::read_to_string(done).unwrap()
}

fn parse(text: &str) -> (hod_ir::Module, Vec<hod_ir::Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        bodies.push(parsed.body.expect("a body hod_ll reads").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    (module, bodies)
}

/// hod's object of `text` by the target's code generator (x86-64's
/// `musttail` calls are declined by the facade's classifier, which has
/// no x86-64 stack-argument rule yet; the target compiles them).
fn hod_object(scratch: &Scratch, arch: Arch, name: &str, text: &str) -> PathBuf {
    hod_object_with(scratch, arch, name, text, &hod_cg::Options::default())
}

fn hod_object_with(
    scratch: &Scratch,
    arch: Arch,
    name: &str,
    text: &str,
    options: &hod_cg::Options,
) -> PathBuf {
    let (module, bodies) = parse(text);
    let bytes = match arch {
        Arch::A64 => hod_a64::compile_to_object(&module, &bodies, options)
            .unwrap_or_else(|e| panic!("{name}: {e:?}")),
        Arch::X64 => hod_x64::compile_to_object(&module, &bodies, options)
            .unwrap_or_else(|e| panic!("{name}: {e:?}")),
    };
    scratch.write(&format!("{name}.o"), bytes)
}

fn llc_object(scratch: &Scratch, llc: &Path, name: &str, text: &str) -> PathBuf {
    llc_object_with(scratch, llc, name, text, &[])
}

fn llc_object_with(
    scratch: &Scratch,
    llc: &Path,
    name: &str,
    text: &str,
    extra: &[&str],
) -> PathBuf {
    let input = scratch.write(&format!("{name}.ll"), text);
    let out = scratch.join(format!("{name}.o"));
    output(
        Command::new(llc)
            .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
            .args(extra)
            .arg(&input)
            .arg("-o")
            .arg(&out),
    );
    out
}

fn verify(dwarfdump: &Path, object: &Path) {
    let mut command = Command::new(dwarfdump);
    command.arg("--verify").arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{} --verify: {}", object.display(), outcome.stdout_text());
}

/// §6.7's masked values, and §6.19's: the return and branch addresses
/// (checked against the code instead) and the target register (hod's
/// call register is not `llc`'s).
const MASKED: [&str; 17] = [
    "DW_AT_low_pc",
    "DW_AT_high_pc",
    "DW_AT_location",
    "DW_AT_frame_base",
    "DW_AT_ranges",
    "DW_AT_addr_base",
    "DW_AT_rnglists_base",
    "DW_AT_str_offsets_base",
    "DW_AT_loclists_base",
    "DW_AT_stmt_list",
    "DW_AT_object_pointer",
    "DW_AT_producer",
    "DW_AT_abstract_origin",
    "DW_AT_call_return_pc",
    "DW_AT_call_pc",
    "DW_AT_call_target_clobbered",
    "DW_AT_GNU_call_site_target_clobbered",
];

/// The normalized DIE tree of `object`, one line per DIE or attribute
/// (depth kept): references by their target's name, constants in
/// decimal. `llc` 19's `DW_AT_call_target` is `llc` 22's
/// `DW_AT_call_target_clobbered` (§6.7).
fn tree(dwarfdump: &Path, object: &Path) -> Vec<String> {
    let text = output(Command::new(dwarfdump).arg("--debug-info").arg(object));
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some((addr, rest)) = line.split_once(':')
            && addr.starts_with("0x")
            && !addr.contains(' ')
        {
            let depth = rest.len() - rest.trim_start().len();
            let tag = rest.trim();
            if tag.starts_with("DW_TAG_") || tag == "NULL" {
                out.push(format!("{depth:>3} {tag}"));
            }
            continue;
        }
        if !trimmed.starts_with("DW_AT_") {
            continue;
        }
        let (name, value) = trimmed.split_once(char::is_whitespace).unwrap_or((trimmed, ""));
        let name = match name {
            "DW_AT_call_target" => "DW_AT_call_target_clobbered",
            "DW_AT_GNU_call_site_target" => "DW_AT_GNU_call_site_target_clobbered",
            name => name,
        };
        if MASKED.contains(&name) {
            out.push(format!("    {name}"));
            continue;
        }
        let value = value.trim();
        if name == "DW_AT_call_origin" && !value.contains('"') {
            // An empty callee DIE (§6.21): its offset depends on the
            // abbreviation codes' lengths; the DIE is compared in place.
            out.push(format!("    {name} (an unnamed DIE)"));
            continue;
        }
        let mut v = value.to_owned();
        if let Some(rest) = value.strip_prefix("(0x")
            && let Some(q) = rest.find(" \"")
        {
            v = format!("({}", &rest[q + 1..]);
        }
        if let Some(num) = value.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
            let parsed = num
                .strip_prefix("0x")
                .map_or_else(|| num.parse::<u64>().ok(), |h| u64::from_str_radix(h, 16).ok());
            if let Some(n) = parsed {
                v = format!("({n})");
            }
        }
        out.push(format!("    {name} {v}"));
    }
    out
}

fn first_difference(hod: &[String], llc: &[String]) -> Option<String> {
    let i = hod
        .iter()
        .zip(llc)
        .position(|(a, b)| a != b)
        .or_else(|| (hod.len() != llc.len()).then_some(hod.len().min(llc.len())))?;
    let from = i.saturating_sub(6);
    let show = |t: &[String]| t[from.min(t.len())..(i + 6).min(t.len())].join("\n");
    Some(format!("at line {i}:\n--- hod\n{}\n--- llc\n{}", show(hod), show(llc)))
}

/// A call site as the dump prints it.
#[derive(Debug, Default)]
struct Site {
    origin: Option<String>,
    target: Option<u16>,
    return_pc: Option<u64>,
    tail: bool,
}

fn hex(v: &str) -> u64 {
    let v = v.trim().trim_start_matches('(').trim_end_matches(')');
    u64::from_str_radix(v.trim_start_matches("0x"), 16).unwrap()
}

fn sites(dwarfdump: &Path, object: &Path) -> Vec<Site> {
    let text = output(Command::new(dwarfdump).arg("--debug-info").arg(object));
    let mut out: Vec<Site> = Vec::new();
    let mut open = false;
    for line in text.lines() {
        let t = line.trim();
        if t.contains(": ") && t.starts_with("0x") || t.ends_with("NULL") {
            open = t.ends_with("DW_TAG_GNU_call_site");
            if open {
                out.push(Site::default());
            }
            continue;
        }
        if !open {
            continue;
        }
        let Some(site) = out.last_mut() else { continue };
        let (name, value) = t.split_once(char::is_whitespace).unwrap_or((t, ""));
        match name {
            "DW_AT_abstract_origin" => {
                site.origin = value.split('"').nth(1).map(str::to_owned);
            }
            // A call's return address; a tail call's branch end.
            "DW_AT_low_pc" => site.return_pc = Some(hex(value)),
            "DW_AT_GNU_tail_call" => site.tail = true,
            "DW_AT_GNU_call_site_target" | "DW_AT_GNU_call_site_target_clobbered" => {
                let op = value.trim().trim_start_matches('(');
                let n = op.strip_prefix("DW_OP_reg").and_then(|r| r.split(' ').next());
                site.target = n.and_then(|n| n.parse().ok());
            }
            _ => {}
        }
    }
    out
}

/// One instruction of `.text`: its address, end, text and relocation.
#[derive(Debug)]
struct Insn {
    addr: u64,
    end: u64,
    text: String,
    reloc: Option<String>,
}

fn disassemble(objdump: &Path, object: &Path) -> Vec<Insn> {
    let text = output(Command::new(objdump).args(["-dr", "-j", ".text"]).arg(object));
    let mut out: Vec<Insn> = Vec::new();
    for line in text.lines() {
        let Some((addr, rest)) = line.split_once(':') else { continue };
        let addr = addr.trim();
        if addr.is_empty() || !addr.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let addr = u64::from_str_radix(addr, 16).unwrap();
        let rest = rest.trim();
        if let Some(r) = rest.strip_prefix("R_") {
            let sym = r.split_whitespace().nth(1).unwrap_or("");
            let sym = sym.split(['+', '-']).next().unwrap_or("").to_owned();
            if let Some(insn) = out.iter_mut().rev().find(|i| i.addr <= addr) {
                insn.reloc = Some(sym);
            }
            continue;
        }
        // The bytes, then the instruction.
        let mut bytes = 0;
        let mut words = rest.split_whitespace().peekable();
        while let Some(w) = words.peek() {
            if w.len() % 2 == 0 && w.len() <= 8 && w.bytes().all(|b| b.is_ascii_hexdigit()) {
                bytes += w.len() / 2;
                words.next();
            } else {
                break;
            }
        }
        let insn: Vec<&str> = words.collect();
        if insn.is_empty() {
            // A continuation of the previous instruction's bytes.
            if let Some(last) = out.last_mut() {
                last.end += bytes as u64;
            }
            continue;
        }
        out.push(Insn { addr, end: addr + bytes as u64, text: insn.join(" "), reloc: None });
    }
    out
}

/// Whether relocation symbol `sym` is the function named `name` (its
/// own name in C, a mangled name holding it in C++).
fn names(sym: &str, name: &str) -> bool {
    sym == name || sym.contains(&format!("{}{name}", name.len()))
}

/// The register an indirect call goes through, by its DWARF number.
fn register(arch: Arch, n: u16) -> String {
    match arch {
        Arch::A64 => format!("x{n}"),
        Arch::X64 => {
            const X64: [&str; 16] = [
                "rax", "rdx", "rcx", "rbx", "rsi", "rdi", "rbp", "rsp", "r8", "r9", "r10", "r11",
                "r12", "r13", "r14", "r15",
            ];
            format!("%{}", X64[usize::from(n)])
        }
    }
}

/// Every call site of `object` against its code; the number checked, and
/// the origins and tail flags in order.
fn check_addresses(
    arch: Arch,
    dwarfdump: &Path,
    objdump: &Path,
    object: &Path,
) -> (usize, Vec<(Option<String>, bool)>) {
    let code = disassemble(objdump, object);
    let all = sites(dwarfdump, object);
    let object = object.display();
    let (calls, branches): (&[&str], &[&str]) = match arch {
        Arch::A64 => (&["bl ", "blr "], &["b ", "br "]),
        Arch::X64 => (&["call"], &["jmp"]),
    };
    let is = |insn: &Insn, mnemonics: &[&str]| {
        mnemonics.iter().any(|m| insn.text.starts_with(m) || insn.text == m.trim())
    };
    for site in &all {
        // DWARF 4 names the end of the instruction for both: a call's
        // return address, a tail call's branch end (`spec/dwarf.md` §6.21).
        let pc = site.return_pc.unwrap_or_else(|| panic!("{object}: no low_pc: {site:?}"));
        let insn = code.iter().find(|i| i.end == pc);
        let insn = insn.unwrap_or_else(|| panic!("{object}: no instruction ends at {pc:#x}"));
        let kind = if site.tail { branches } else { calls };
        assert!(is(insn, kind), "{object}: {site:?} after {insn:?}, not the right kind");
        match (&site.origin, site.target) {
            (Some(name), None) => {
                let sym = insn.reloc.as_deref().unwrap_or("");
                assert!(names(sym, name), "{object}: {site:?} calls {insn:?}");
            }
            (None, Some(n)) => {
                assert!(insn.reloc.is_none(), "{object}: {site:?} is direct: {insn:?}");
                let reg = register(arch, n);
                assert!(insn.text.contains(&reg), "{object}: {site:?} through {reg}: {insn:?}");
            }
            _ => panic!("{object}: a call site with neither or both: {site:?}"),
        }
    }
    (all.len(), all.iter().map(|s| (s.origin.clone(), s.tail)).collect())
}

/// hod's and `llc`'s trees and addresses for each source, front end and
/// target.
#[test]
fn trees_equal_llc() {
    let (Some(dwarfdump), Some(objdump)) = (tool("llvm-dwarfdump"), tool("llvm-objdump")) else {
        return;
    };
    let scratch = Scratch::new("call-sites4-trees");
    let mut compared = 0;
    for source in ["calls.c", "calls.cpp", "tail.c"] {
        for fe in frontends(Path::new(source).extension().is_some_and(|e| e == "cpp")) {
            for arch in ARCHES {
                let tag = format!("{source}-{}-{arch:?}", fe.name);
                let text = ir(&fe, &scratch, arch, source, &[]);
                assert!(text.contains("!\"Dwarf Version\", i32 4}"), "{tag}");
                compare(&scratch, &dwarfdump, &objdump, &fe.llc, arch, &tag, &text);
                compared += 1;
            }
        }
    }
    eprintln!("{compared} trees compared");
}

/// One comparison: trees, `--verify`, sites; returns hod's site count.
fn compare(
    scratch: &Scratch,
    dwarfdump: &Path,
    objdump: &Path,
    llc: &Path,
    arch: Arch,
    tag: &str,
    text: &str,
) -> usize {
    let hod = hod_object(scratch, arch, &format!("{tag}-hod"), text);
    let llc = llc_object(scratch, llc, &format!("{tag}-llc"), text);
    verify(dwarfdump, &hod);
    verify(dwarfdump, &llc);
    let (h, l) = (tree(dwarfdump, &hod), tree(dwarfdump, &llc));
    if let Some(d) = first_difference(&h, &l) {
        panic!("{tag}: {d}");
    }
    let sites = check_addresses(arch, dwarfdump, objdump, &hod);
    let llc_sites = check_addresses(arch, dwarfdump, objdump, &llc);
    assert_eq!(sites, llc_sites, "{tag}");
    assert!(sites.0 > 0, "{tag}: no call site");
    assert!(h.iter().any(|l| l.ends_with("DW_AT_GNU_all_call_sites (true)")), "{tag}");
    sites.0
}

/// A module without `Dwarf Version` is version 4 for `llc` and for hod
/// (§6.20): the same call sites.
#[test]
fn flagless_module() {
    let (Some(dwarfdump), Some(objdump)) = (tool("llvm-dwarfdump"), tool("llvm-objdump")) else {
        return;
    };
    let (Some(clang), Some(opt), Some(llc)) = (tool("clang-22"), tool("opt-22"), tool("llc-22"))
    else {
        return;
    };
    let fe = Frontend { name: "c22", compiler: clang, opt: Some(opt), llc };
    let scratch = Scratch::new("call-sites4-flagless");
    for arch in ARCHES {
        let text = ir(&fe, &scratch, arch, "calls.c", &[]);
        let text = without_version(&text);
        let tag = format!("flagless-{arch:?}");
        let n = compare(&scratch, &dwarfdump, &objdump, &fe.llc, arch, &tag, &text);
        assert!(n >= 4, "{tag}: {n}");
    }
}

/// `text` without its `Dwarf Version` flag (the node and its use).
fn without_version(text: &str) -> String {
    let line = text.lines().find(|l| l.contains("!\"Dwarf Version\"")).expect("the flag");
    let node = line.split_once(" = ").expect("a node").0;
    let mut out = String::with_capacity(text.len());
    for l in text.lines() {
        if l == line {
            continue;
        }
        if l.starts_with("!llvm.module.flags = ") {
            let l = l.replace(&format!("{node}, "), "").replace(&format!(", {node}}}"), "}");
            out.push_str(&l);
        } else {
            out.push_str(l);
        }
        out.push('\n');
    }
    assert!(!out.contains("Dwarf Version"));
    out
}

/// The call sites a dump lists at either version, by tag: `(origin
/// name or "", tail)` in order.
fn any_version_sites(dwarfdump: &Path, object: &Path) -> Vec<String> {
    let text = output(Command::new(dwarfdump).arg("--debug-info").arg(object));
    let mut out = Vec::new();
    let mut open = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("0x") && t.contains(": ") || t.ends_with("NULL") {
            open = t.ends_with("DW_TAG_call_site") || t.ends_with("DW_TAG_GNU_call_site");
            if open {
                out.push(String::new());
            }
            continue;
        }
        if open
            && (t.starts_with("DW_AT_call_origin") || t.starts_with("DW_AT_abstract_origin"))
            && let Some(name) = t.split('"').nth(1)
            && let Some(last) = out.last_mut()
        {
            last.push_str(name);
        }
    }
    out
}

/// TLS call paths (WP-150 item 6): `llc` writes no call site for the
/// model's own calls (`__tls_get_addr` on x86-64, the descriptor's `blr`
/// on AArch64) in any of the four models, and neither does hod; the call
/// of `ext` has its site. PIC, so that `llc` keeps each model.
#[test]
fn thread_local_models() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let scratch = Scratch::new("call-sites4-tls");
    let pic = hod_cg::Options { reloc_model: hod_cg::RelocModel::Pic, ..Default::default() };
    let mut compared = 0;
    for fe in frontends(false) {
        // kiln++ has no `-ftls-model`: its default, global-dynamic.
        let kiln = fe.opt.is_none();
        for arch in ARCHES {
            for model in ["global-dynamic", "local-dynamic", "initial-exec", "local-exec"] {
                if kiln && model != "global-dynamic" {
                    continue;
                }
                for version in ["-gdwarf-4", "-gdwarf-5"] {
                    let flag = format!("-ftls-model={model}");
                    let flags: &[&str] =
                        if kiln { &[version, "-fPIC"] } else { &[&flag, version, "-fPIC"] };
                    let text = ir(&fe, &scratch, arch, "tls.c", flags);
                    let tag = format!("tls-{}-{arch:?}-{model}{version}", fe.name);
                    let ir_model = match model {
                        "global-dynamic" => "thread_local global",
                        "local-dynamic" => "thread_local(localdynamic)",
                        "initial-exec" => "thread_local(initialexec)",
                        _ => "thread_local(localexec)",
                    };
                    assert!(text.contains(ir_model), "{tag}");
                    let hod = hod_object_with(&scratch, arch, &format!("{tag}-hod"), &text, &pic);
                    let llc = llc_object_with(
                        &scratch,
                        &fe.llc,
                        &format!("{tag}-llc"),
                        &text,
                        &["-relocation-model=pic"],
                    );
                    verify(&dwarfdump, &hod);
                    verify(&dwarfdump, &llc);
                    let (h, l) =
                        (any_version_sites(&dwarfdump, &hod), any_version_sites(&dwarfdump, &llc));
                    assert_eq!(h, l, "{tag}: the call sites");
                    // kiln's declarations have no subprogram: no site.
                    let expect: &[&str] = if kiln { &[] } else { &["ext"] };
                    assert_eq!(h, expect, "{tag}: the call sites");
                    compared += 1;
                }
            }
        }
    }
    eprintln!("{compared} TLS objects compared");
}

/// `-gline-tables-only` call sites (WP-150 item 5, §6.21), at versions 5
/// and 4, by clang 19 and 22, on both targets: hod's tree is `llc`'s
/// (a minimal subprogram with `DW_AT_call_all_calls`, its call sites, the
/// callees' declarations and the empty DIEs of callees without a function
/// DIE), both `--verify` clean.
#[test]
fn line_tables_only_matches_llc() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let scratch = Scratch::new("call-sites4-gmlt");
    let mut compared = 0;
    let mut empty = 0;
    for (source, cxx) in [("calls.c", false), ("calls.cpp", true)] {
        for fe in frontends(cxx).into_iter().filter(|fe| fe.opt.is_some()) {
            for arch in ARCHES {
                for version in ["-gdwarf-5", "-gdwarf-4"] {
                    let flags = [version, "-gline-tables-only"];
                    let text = ir(&fe, &scratch, arch, source, &flags);
                    assert!(text.contains("emissionKind: LineTablesOnly"));
                    let tag = format!("gmlt-{source}-{}-{arch:?}{version}", fe.name);
                    let hod = hod_object(&scratch, arch, &format!("{tag}-hod"), &text);
                    let llc = llc_object(&scratch, &fe.llc, &format!("{tag}-llc"), &text);
                    verify(&dwarfdump, &hod);
                    verify(&dwarfdump, &llc);
                    let (h, l) = (tree(&dwarfdump, &hod), tree(&dwarfdump, &llc));
                    if let Some(d) = first_difference(&h, &l) {
                        panic!("{tag}: {d}");
                    }
                    let sites = any_version_sites(&dwarfdump, &hod);
                    assert!(sites.len() >= 3, "{tag}: {sites:?}");
                    assert_eq!(sites, any_version_sites(&dwarfdump, &llc), "{tag}");
                    // A callee without a function DIE: an empty DIE.
                    empty += h
                        .windows(2)
                        .filter(|w| {
                            w[0].ends_with("DW_TAG_subprogram") && !w[1].starts_with("    ")
                        })
                        .count();
                    compared += 1;
                }
            }
        }
    }
    assert!(compared == 0 || empty > 0, "no empty callee DIE");
    eprintln!("{compared} line-tables-only trees compared, {empty} empty callee DIEs");
}
