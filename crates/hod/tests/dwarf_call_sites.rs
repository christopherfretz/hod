//! Call-site entries (WP-113, `spec/dwarf.md` §6.19): for a subprogram
//! with `DIFlagAllCallsDescribed`, `DW_AT_call_all_calls` and one
//! `DW_TAG_call_site` per call, on AArch64 and x86-64.
//!
//! - `trees_equal_llc`: the DIE trees of `tests/data/call_sites/*` by
//!   clang 19 (`llc` 19), clang 22 and kiln++ (`llc` 22; the flag added
//!   to kiln's `-O0` definitions, which never set it) equal `llc`'s after
//!   §6.7's normalization, both objects `--verify` clean, and every
//!   `call_return_pc` and `call_pc` of BOTH objects is checked against
//!   the disassembly: the instruction ending at the return address is a
//!   call of the origin (or through the target register), the one at a
//!   tail call's `call_pc` a branch to it.
//! - the refusals by name: an inlined call in a line-tables-only unit.
//! - split DWARF: the call sites go to the `.dwo`.
//!
//! clang sets the flag only when optimizing, so the C inputs are
//! `-g -O1 -Xclang -disable-llvm-passes` (the code stays `-O0`'s shape),
//! with `opt -passes=always-inline` for the inlined calls.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod::{CompileError, Construct, OutputKind, Report, Settings, compile_with};
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
    let tag = format!("{source}-{}-{arch:?}", fe.name);
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
        command.args(["-g", "-O0", "-std=c++20"]);
    } else {
        command.args(["-g", "-O1", "-Xclang", "-disable-llvm-passes"]);
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
    let (module, bodies) = parse(text);
    let options = hod_cg::Options::default();
    let bytes = match arch {
        Arch::A64 => hod_a64::compile_to_object(&module, &bodies, &options)
            .unwrap_or_else(|e| panic!("{name}: {e:?}")),
        Arch::X64 => hod_x64::compile_to_object(&module, &bodies, &options)
            .unwrap_or_else(|e| panic!("{name}: {e:?}")),
    };
    scratch.write(&format!("{name}.o"), bytes)
}

fn llc_object(scratch: &Scratch, llc: &Path, name: &str, text: &str) -> PathBuf {
    let input = scratch.write(&format!("{name}.ll"), text);
    let out = scratch.join(format!("{name}.o"));
    output(
        Command::new(llc)
            .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
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
const MASKED: [&str; 16] = [
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
        let name = if name == "DW_AT_call_target" { "DW_AT_call_target_clobbered" } else { name };
        if MASKED.contains(&name) {
            out.push(format!("    {name}"));
            continue;
        }
        let value = value.trim();
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
    call_pc: Option<u64>,
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
            open = t.ends_with("DW_TAG_call_site");
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
            "DW_AT_call_origin" => {
                site.origin = value.split('"').nth(1).map(str::to_owned);
            }
            "DW_AT_call_return_pc" => site.return_pc = Some(hex(value)),
            "DW_AT_call_pc" => site.call_pc = Some(hex(value)),
            "DW_AT_call_tail_call" => site.tail = true,
            "DW_AT_call_target" | "DW_AT_call_target_clobbered" => {
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

/// Every call site of `object` against its code; the number checked.
fn check_addresses(arch: Arch, dwarfdump: &Path, objdump: &Path, object: &Path) -> usize {
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
        let insn = if site.tail {
            assert!(site.return_pc.is_none(), "{object}: a tail call with a return address");
            let pc = site.call_pc.unwrap_or_else(|| panic!("{object}: no call_pc: {site:?}"));
            let insn = code.iter().find(|i| i.addr == pc);
            let insn = insn.unwrap_or_else(|| panic!("{object}: no instruction at {pc:#x}"));
            assert!(is(insn, branches), "{object}: {site:?} at {insn:?}, not a branch");
            insn
        } else {
            let pc = site.return_pc.unwrap_or_else(|| panic!("{object}: no return_pc: {site:?}"));
            let insn = code.iter().find(|i| i.end == pc);
            let insn = insn.unwrap_or_else(|| panic!("{object}: no instruction ends at {pc:#x}"));
            assert!(is(insn, calls), "{object}: {site:?} after {insn:?}, not a call");
            insn
        };
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
    all.len()
}

/// hod's and `llc`'s trees and addresses for each source, front end and
/// target.
#[test]
fn trees_equal_llc() {
    let (Some(dwarfdump), Some(objdump)) = (tool("llvm-dwarfdump"), tool("llvm-objdump")) else {
        return;
    };
    let scratch = Scratch::new("call-sites-trees");
    let mut compared = 0;
    for source in ["calls.c", "calls.cpp", "tail.c"] {
        for fe in frontends(Path::new(source).extension().is_some_and(|e| e == "cpp")) {
            for arch in ARCHES {
                let tag = format!("{source}-{}-{arch:?}", fe.name);
                let text = ir(&fe, &scratch, arch, source, &[]);
                let hod = hod_object(&scratch, arch, &format!("{tag}-hod"), &text);
                let llc = llc_object(&scratch, &fe.llc, &format!("{tag}-llc"), &text);
                verify(&dwarfdump, &hod);
                verify(&dwarfdump, &llc);
                let (h, l) = (tree(&dwarfdump, &hod), tree(&dwarfdump, &llc));
                if let Some(d) = first_difference(&h, &l) {
                    panic!("{tag}: {d}");
                }
                let sites = check_addresses(arch, &dwarfdump, &objdump, &hod);
                let llc_sites = check_addresses(arch, &dwarfdump, &objdump, &llc);
                assert_eq!(sites, llc_sites, "{tag}");
                assert!(sites > 0, "{tag}: no call site");
                assert!(h.iter().any(|l| l.ends_with("DW_AT_call_all_calls (true)")), "{tag}");
                compared += 1;
            }
        }
    }
    eprintln!("{compared} trees compared");
}

fn declined(text: &str, options: &hod::Options) -> Result<Vec<u8>, String> {
    match compile_with(text.as_bytes(), options, &Settings::default(), &mut Report::default()) {
        Ok(object) => Ok(object),
        Err(CompileError::Unsupported(u)) => match u.construct {
            Construct::Named(name) => Err(name.into_owned()),
            other => panic!("declined, but not by name: {other:?}"),
        },
        Err(other) => panic!("expected an object or a decline, got {other:?}"),
    }
}

fn options() -> hod::Options {
    hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() }
}

/// `-gline-tables-only` (`-O1`): a unit with no inlined code has no
/// function DIE (as `llc`), the flag is accepted; with inlined code
/// hod writes `llc`'s call sites (WP-150).
#[test]
fn line_tables_only() {
    let Some(clang) = tool("clang-22") else { return };
    let Some(opt) = tool("opt-22") else { return };
    let Some(llc) = tool("llc-22") else { return };
    let fe = Frontend { name: "gmlt", compiler: clang, opt: Some(opt), llc };
    let scratch = Scratch::new("call-sites-gmlt");
    let flat = ir(&fe, &scratch, Arch::A64, "tail.c", &["-gline-tables-only"]);
    assert!(flat.contains("DIFlagAllCallsDescribed"));
    let object = declined(&flat, &options()).expect("a line-tables-only unit without inlining");
    let object = scratch.write("flat.o", object);
    if let Some(dwarfdump) = tool("llvm-dwarfdump") {
        verify(&dwarfdump, &object);
        let tree = tree(&dwarfdump, &object);
        assert!(!tree.iter().any(|l| l.ends_with("DW_TAG_call_site")), "{tree:?}");
    }
    // WP-150: with inlined code, `llc`'s minimal call sites
    // (`dwarf_call_sites4.rs`, `line_tables_only_matches_llc`).
    let inlined = ir(&fe, &scratch, Arch::A64, "calls.c", &["-gline-tables-only"]);
    declined(&inlined, &options()).expect("line-tables-only call sites");
}

/// `-gsplit-dwarf`: the call sites are in the `.dwo`, their addresses in
/// the skeleton's `.debug_addr`; both `--verify` clean.
#[test]
fn split_dwarf() {
    let (Some(clang), Some(opt), Some(llc)) = (tool("clang-22"), tool("opt-22"), tool("llc-22"))
    else {
        return;
    };
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let fe = Frontend { name: "split", compiler: clang, opt: Some(opt), llc };
    let scratch = Scratch::new("call-sites-split");
    let text = ir(&fe, &scratch, Arch::A64, "calls.cpp", &["-gsplit-dwarf"]);
    let dwo = scratch.join("calls.dwo");
    let options = hod::Options {
        split_dwarf_file: Some(PathBuf::from("calls.dwo")),
        split_dwarf_output: Some(dwo.clone()),
        ..options()
    };
    let object = scratch.write("calls.o", declined(&text, &options).expect("split DWARF"));
    verify(&dwarfdump, &object);
    verify(&dwarfdump, &dwo);
    // `llc`'s `.dwo` of the same IR has as many.
    let llc_dwo = scratch.join("calls-llc.dwo");
    let llc_object = scratch.join("calls-llc.o");
    let input = scratch.write("calls-llc.ll", &text);
    output(
        Command::new(&fe.llc)
            .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
            .arg("-split-dwarf-file=calls-llc.dwo")
            .arg(format!("-split-dwarf-output={}", llc_dwo.display()))
            .arg(&input)
            .arg("-o")
            .arg(&llc_object),
    );
    let count = |dwo: &Path| {
        tree(&dwarfdump, dwo).iter().filter(|l| l.ends_with("DW_TAG_call_site")).count()
    };
    let n = count(&dwo);
    assert!(n > 0);
    assert_eq!(n, count(&llc_dwo), "call sites in hod's .dwo and in llc's");
}

/// A module of `n` functions of `calls` calls each, flagged or not.
fn synthetic(n: usize, calls: usize, flagged: bool) -> String {
    use std::fmt::Write;
    let mut t = String::new();
    t.push_str(
        "target triple = \"aarch64-unknown-linux-gnu\"\n\ndeclare !dbg !5 void @ext(i32) #0\n\n",
    );
    for f in 0..n {
        writeln!(t, "define void @f{f}() #0 !dbg !{} {{", 100 + 2 * f).unwrap();
        for c in 0..calls {
            writeln!(t, "  call void @ext(i32 {c}), !dbg !{}", 101 + 2 * f).unwrap();
        }
        t.push_str("  ret void\n}\n\n");
    }
    t.push_str("attributes #0 = { noinline nounwind optnone \"frame-pointer\"=\"non-leaf\" }\n");
    t.push_str("!llvm.dbg.cu = !{!0}\n!llvm.module.flags = !{!2, !3}\n");
    t.push_str("!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: \"p\", isOptimized: true, runtimeVersion: 0, emissionKind: FullDebug)\n");
    t.push_str("!1 = !DIFile(filename: \"s.c\", directory: \"/\")\n");
    t.push_str(
        "!2 = !{i32 7, !\"Dwarf Version\", i32 5}\n!3 = !{i32 2, !\"Debug Info Version\", i32 3}\n",
    );
    t.push_str("!4 = !DISubroutineType(types: !{null})\n");
    t.push_str("!5 = !DISubprogram(name: \"ext\", scope: !1, file: !1, line: 1, type: !4, flags: DIFlagPrototyped, spFlags: DISPFlagOptimized)\n");
    let flags =
        if flagged { "DIFlagPrototyped | DIFlagAllCallsDescribed" } else { "DIFlagPrototyped" };
    for f in 0..n {
        writeln!(t, "!{} = distinct !DISubprogram(name: \"f{f}\", scope: !1, file: !1, line: {}, type: !4, scopeLine: 1, flags: {flags}, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !0)", 100 + 2 * f, f + 1).unwrap();
        writeln!(t, "!{} = !DILocation(line: {}, scope: !{})", 101 + 2 * f, f + 1, 100 + 2 * f)
            .unwrap();
    }
    t
}

/// The cost of a call-site DIE, at `N` and `2N` (ignored: wall time;
/// `cargo test --release -p hod --test dwarf_call_sites -- --ignored
/// --nocapture throughput`). Not asserted.
#[test]
#[ignore = "measures wall time; run by hand"]
#[allow(clippy::cast_precision_loss, reason = "a printed measurement")]
fn throughput() {
    let time = |text: &str| {
        let (module, bodies) = parse(text);
        let options = hod_cg::Options::default();
        let mut best = Duration::MAX;
        for _ in 0..5 {
            let start = std::time::Instant::now();
            let object = hod_a64::compile_to_object(&module, &bodies, &options).unwrap();
            best = best.min(start.elapsed());
            std::hint::black_box(object);
        }
        best
    };
    for n in [2000, 4000] {
        let calls = 8;
        let (with, without) = (time(&synthetic(n, calls, true)), time(&synthetic(n, calls, false)));
        let sites = (n * calls) as f64;
        let extra = with.as_nanos() as f64 - without.as_nanos() as f64;
        eprintln!(
            "N={n} functions, {sites} call sites: {:.2} ms flagged, {:.2} ms not, {:.1} ns per call-site DIE",
            with.as_secs_f64() * 1e3,
            without.as_secs_f64() * 1e3,
            extra / sites
        );
    }
}

/// The synthetic module compiles, `--verify` clean, with one call site
/// per call.
#[test]
fn synthetic_sites() {
    let scratch = Scratch::new("call-sites-synthetic");
    let (module, bodies) = parse(&synthetic(3, 4, true));
    let object = hod_a64::compile_to_object(&module, &bodies, &hod_cg::Options::default()).unwrap();
    let object = scratch.write("s.o", object);
    let (Some(dwarfdump), Some(objdump)) = (tool("llvm-dwarfdump"), tool("llvm-objdump")) else {
        return;
    };
    verify(&dwarfdump, &object);
    assert_eq!(check_addresses(Arch::A64, &dwarfdump, &objdump, &object), 12);
    let tree = tree(&dwarfdump, &object);
    assert_eq!(tree.iter().filter(|l| l.ends_with("DW_AT_call_all_calls (true)")).count(), 3);
}
