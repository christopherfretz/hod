//! `-fcf-protection` on x86-64 (WP-184, `spec/codegen.md` §5.6.25): the
//! module flags `cf-protection-branch` and `cf-protection-return`, the
//! `endbr64` landing instructions (`crate::cet`) and the property note.
//!
//! - Shape against `llc -O0` 19 and 22: per function, whether its entry
//!   is marked, after which direct calls an `endbr64` follows, and how
//!   many other `endbr64` it has. hod differs where it says so: it marks
//!   the entry of a local function whose address is never taken
//!   (`@l_direct`), and it marks an `invoke`'s pad stub besides the pad
//!   (`@catch_stub`, the one `invoke` with stack arguments).
//! - Consumers: in hod's object (and, as a check of the check, `llc`'s)
//!   every landing address of the `.gcc_except_table` call-site tables,
//!   every code address held in data (function pointers, block
//!   addresses) and every function symbol but a `nocf_check` one starts
//!   with `endbr64`; the coroutine seeds' entry stubs too.
//! - The note: `.note.gnu.property` byte for byte and header for header
//!   as `llc`'s for each flag set.
//! - Programs: freestanding ones (no C runtime, whose start files carry
//!   no property in the cross system root) of hod's, `llc`'s and mixed
//!   objects, linked by GNU ld (`-z cet-report=error`), lld
//!   (`-z force-ibt`, `-z shstk`) and mold: the program carries IBT (and
//!   SHSTK where every object asks for it), and runs under `qemu-x86_64`,
//!   which does not enforce IBT (the shape and consumer checks carry the
//!   proof that the marks are where an enforcing processor needs them).
//! - Refusals: an indirect `nocf_check` call under `cf-protection-branch`.
#![cfg(target_os = "linux")]

#[path = "../../hod_obj/tests/common/mod.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod objcommon;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{CgError, Options, RelocModel};
use hod_obj::{Arch, Object, SectionKind, Symbol, X64Reloc, elf};
use hod_testutil::{Scratch, run_with, tool};
use objcommon::Elf;
use objcommon::linking::{Toolchain, exit_code};

const MAJORS: [u32; 2] = [22, 19];
const ENDBR64: [u8; 4] = [0xf3, 0x0f, 0x1e, 0xfa];
const SEED: &str = include_str!("../../../tests/ir/cf_protection.ll");

const BRANCH: &str =
    "!llvm.module.flags = !{!0}\n!0 = !{i32 8, !\"cf-protection-branch\", i32 1}\n";
const RETURN: &str =
    "!llvm.module.flags = !{!0}\n!0 = !{i32 8, !\"cf-protection-return\", i32 1}\n";
const FULL: &str = "!llvm.module.flags = !{!0, !1}\n!0 = !{i32 8, !\"cf-protection-return\", i32 1}\n\
                    !1 = !{i32 8, !\"cf-protection-branch\", i32 1}\n";

fn header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// The seed's functions with `flags` (one of the constants, or "") as its
/// module flags.
fn seed(flags: &str) -> String {
    let at = SEED.find("!llvm.module.flags").expect("the seed's flags");
    format!("{}{}{flags}", header(), &SEED[..at])
}

fn parse(text: &str) -> (hod_ir::Module, Vec<hod_ir::Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &parsed.text, |_, parsed| {
        bodies.push(parsed.body.expect("every body is read").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    (module, bodies)
}

fn options() -> Options {
    Options { reloc_model: RelocModel::Pic, function_sections: true, ..Options::default() }
}

fn try_hod(text: &str) -> Result<Vec<u8>, CgError> {
    let (module, bodies) = parse(text);
    match hod_x64::compile_to_object(&module, &bodies, &options()) {
        Ok(object) => Ok(object),
        Err(hod_x64::ModuleError::Function { error, .. }) => Err(error),
        Err(other) => panic!("{other}"),
    }
}

fn hod(text: &str) -> Vec<u8> {
    try_hod(text).unwrap_or_else(|e| panic!("hod: {e}"))
}

fn llc(dir: &Scratch, major: u32, name: &str, text: &str) -> Option<PathBuf> {
    let llc = hod_testutil::find_tool(&format!("llc-{major}"))?;
    let input = dir.write(&format!("{name}-llc{major}.ll"), text);
    let output = dir.join(format!("{name}-llc{major}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-mtriple=x86_64-unknown-linux-gnu", "-relocation-model=pic"])
        .args(["-function-sections", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "llc {major} failed:\n{}", outcome.stderr_text());
    Some(output)
}

// ---- the shape ----

/// Where one function's `endbr64` instructions are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Marks {
    /// At offset 0.
    entry: bool,
    /// After a call: the callee's name (`*` for an indirect call).
    after: Vec<String>,
    /// Anywhere else.
    other: usize,
}

/// The callee of a relocation line's symbol: `rt-0x4` or `.text.rt-0x4`
/// is `rt`.
fn callee(symbol: &str) -> String {
    let name = symbol.split(['-', '+']).next().unwrap_or("");
    name.strip_prefix(".text.").unwrap_or(name).to_owned()
}

/// The marks of each function of `object`, from `llvm-objdump -dr`.
fn marks(object: &Path) -> Option<BTreeMap<String, Marks>> {
    let objdump = tool("llvm-objdump")?;
    let mut command = Command::new(objdump);
    command.args(["-dr", "--no-show-raw-insn"]).arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    // (function, offset, mnemonic, relocation symbol)
    let mut insts: Vec<(String, u64, String, Option<String>)> = Vec::new();
    let mut function = String::new();
    for line in outcome.stdout_text().lines() {
        if let Some(name) = line.split_once(" <").and_then(|(_, r)| r.strip_suffix(">:")) {
            name.clone_into(&mut function);
            continue;
        }
        if line.contains("R_X86_64_") {
            if let Some(last) = insts.last_mut() {
                last.3 = line.split_whitespace().last().map(callee);
            }
            continue;
        }
        let Some((at, rest)) = line.trim_start().split_once(':') else { continue };
        let Ok(at) = u64::from_str_radix(at.trim(), 16) else { continue };
        let mnemonic = rest.split_whitespace().next().unwrap_or("").to_owned();
        if !mnemonic.is_empty() && !function.is_empty() {
            insts.push((function.clone(), at, mnemonic, None));
        }
    }
    let mut out: BTreeMap<String, Marks> = BTreeMap::new();
    for (index, (function, at, mnemonic, _)) in insts.iter().enumerate() {
        let marks = out.entry(function.clone()).or_default();
        if mnemonic != "endbr64" {
            continue;
        }
        let before = index.checked_sub(1).map(|i| &insts[i]).filter(|i| &i.0 == function);
        if *at == 0 {
            marks.entry = true;
        } else if let Some((_, _, call, target)) = before.filter(|i| i.2.starts_with("call")) {
            let _ = call;
            marks.after.push(target.clone().unwrap_or_else(|| "*".to_owned()));
        } else {
            marks.other += 1;
        }
    }
    Some(out)
}

/// The `llvm.eh.sjlj.setjmp` resume point (`cf-protection-return`
/// refuses it, so these run under the branch flag alone).
const SJLJ: &str = "
declare ptr @llvm.frameaddress.p0(i32)
declare ptr @llvm.stacksave.p0()
declare i32 @llvm.eh.sjlj.setjmp(ptr)
declare void @llvm.eh.sjlj.longjmp(ptr)

define internal void @jumper(ptr %b) noinline {
  call void @llvm.eh.sjlj.longjmp(ptr %b)
  unreachable
}

define i32 @sj() noinline {
entry:
  %buf = alloca [5 x ptr], align 16
  %fa = call ptr @llvm.frameaddress.p0(i32 0)
  store ptr %fa, ptr %buf, align 16
  %ss = call ptr @llvm.stacksave.p0()
  %p2 = getelementptr ptr, ptr %buf, i64 2
  store ptr %ss, ptr %p2, align 16
  %r = call i32 @llvm.eh.sjlj.setjmp(ptr %buf)
  %z = icmp eq i32 %r, 0
  br i1 %z, label %first, label %second
first:
  call void @jumper(ptr %buf)
  unreachable
second:
  ret i32 77
}
";

/// The modules the shape is compared on, and whether each tracks
/// indirect branches.
fn shape_modules() -> Vec<(&'static str, String, bool)> {
    vec![
        ("seed-full", seed(FULL), true),
        ("seed-branch", seed(BRANCH), true),
        ("seed-return", seed(RETURN), false),
        ("seed-none", seed(""), false),
        ("sjlj", format!("{}{SJLJ}{BRANCH}", header()), true),
    ]
}

/// hod's marks, as `llc`'s plus what hod adds by design (module doc).
fn expected_hod(name: &str, llc: &Marks, tracks: bool) -> Marks {
    let mut marks = llc.clone();
    // Local functions only called directly.
    if tracks && ["l_direct", "jumper"].contains(&name) {
        assert!(!llc.entry, "llc marks a local function whose address is not taken");
        marks.entry = true;
    }
    if tracks && name == "catch_stub" {
        marks.other += 1;
    }
    marks
}

#[test]
fn endbr64_where_llc_puts_it() {
    let dir = Scratch::new("x64-cet-shape");
    for (name, text, tracks) in shape_modules() {
        let object = dir.write(&format!("{name}-hod.o"), hod(&text));
        let Some(ours) = marks(&object) else { return };
        let mut compared = 0;
        for major in MAJORS {
            let Some(theirs) = llc(&dir, major, name, &text) else { continue };
            let theirs = marks(&theirs).unwrap();
            let names: Vec<_> = theirs.keys().collect();
            assert_eq!(ours.keys().collect::<Vec<_>>(), names, "{name}: llc {major}'s functions");
            for (function, llc_marks) in &theirs {
                let want = expected_hod(function, llc_marks, tracks);
                assert_eq!(ours[function], want, "{name}: {function} against llc {major}");
            }
            compared += 1;
        }
        // The rule `llc` follows, stated on its result too.
        if compared > 0 && tracks && name.starts_with("seed") {
            for (function, entry) in [("nocf_g", false), ("nocf_l", false), ("l_tab", true)] {
                assert_eq!(ours[function].entry, entry, "{name}: {function}");
            }
            assert_eq!(ours["calls"].after, ["rt"], "{name}: after the returns_twice call");
            assert_eq!(ours["ib"].other, 3, "{name}: the three block addresses");
            assert_eq!(ours["catch_phi"].other, 1, "{name}: the landing pad with a phi");
        }
        if !tracks {
            assert!(ours.values().all(|m| *m == Marks::default()), "{name}: no endbr64");
        }
    }
}

// ---- the consumers ----

/// Reads an unsigned LEB128 at `*at`.
fn uleb(bytes: &[u8], at: &mut usize) -> u64 {
    let (mut value, mut shift) = (0u64, 0);
    loop {
        let byte = bytes[*at];
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            return value;
        }
    }
}

/// The landing offsets of an LSDA whose call-site table is in ULEB128
/// (what `llc` and hod write), relative to the function's start.
fn landings(lsda: &[u8]) -> Vec<u64> {
    let mut at = 0;
    let lp_start = lsda[at];
    at += 1;
    assert_eq!(lp_start, 0xff, "no @LPStart");
    let ttype = lsda[at];
    at += 1;
    if ttype != 0xff {
        uleb(lsda, &mut at);
    }
    assert_eq!(lsda[at], 1, "call-site encoding ULEB128");
    at += 1;
    let length = uleb(lsda, &mut at) as usize;
    let end = at + length;
    let mut out = Vec::new();
    while at < end {
        let (_start, _len) = (uleb(lsda, &mut at), uleb(lsda, &mut at));
        let landing = uleb(lsda, &mut at);
        uleb(lsda, &mut at);
        if landing != 0 {
            out.push(landing);
        }
    }
    out
}

/// Whether the code of section `section` at `offset` is `endbr64`.
fn marked(elf: &Elf, section: usize, offset: u64) -> bool {
    elf.data(section).get(offset as usize..offset as usize + 4) == Some(&ENDBR64[..])
}

/// Every landing pad, code address in data and function symbol (but
/// those named in `unmarked`) of `object` is marked; the count of each.
fn consumers_marked(object: &[u8], unmarked: &[&str], label: &str) -> [usize; 3] {
    let elf = Elf::parse(object);
    let mut counts = [0; 3];
    for (index, section) in elf.sections.iter().enumerate() {
        let Some(function) = section.name.strip_prefix(".gcc_except_table.") else { continue };
        let text = elf.only(&format!(".text.{function}"));
        for landing in landings(elf.data(index)) {
            assert!(marked(&elf, text, landing), "{label}: pad {function}+{landing:#x}");
            counts[0] += 1;
        }
    }
    let is_text = |index: usize| elf.sections[index].name.starts_with(".text");
    for (index, section) in elf.sections.iter().enumerate() {
        let target = section.info as usize;
        if section.kind != objcommon::SHT_RELA
            || is_text(target)
            || [".eh_frame", ".gcc_except_table", ".debug"]
                .iter()
                .any(|p| elf.sections[target].name.starts_with(p))
        {
            continue;
        }
        for rela in elf.relas_in(index) {
            let sym = &elf.symbols[rela.sym as usize];
            let shndx = sym.shndx as usize;
            if sym.shndx == 0 || shndx >= elf.sections.len() || !is_text(shndx) {
                continue;
            }
            let at = sym.value.wrapping_add_signed(rela.addend);
            assert!(marked(&elf, shndx, at), "{label}: {}+{at:#x} held in data", sym.name);
            counts[1] += 1;
        }
    }
    for sym in &elf.symbols {
        let shndx = sym.shndx as usize;
        if sym.kind != 2 || sym.shndx == 0 || unmarked.contains(&sym.name.as_str()) {
            continue;
        }
        assert!(marked(&elf, shndx, sym.value), "{label}: function {}", sym.name);
        counts[2] += 1;
    }
    counts
}

#[test]
fn every_indirect_branch_target_is_marked() {
    let unmarked = ["nocf_g", "nocf_l"];
    for flags in [FULL, BRANCH] {
        let text = seed(flags);
        let counts = consumers_marked(&hod(&text), &unmarked, "hod");
        // Five call-site records with a pad (the two `invoke`s of
        // `@catch_phi` share one; `@catch_stub`'s lands in its stub);
        // three block addresses and one in `@ba_data`, `@l_tab`, `@g_ext`,
        // `@l_stored`.
        assert_eq!(counts[..2], [5, 7], "hod");
    }
    let sjlj = format!("{}{SJLJ}{BRANCH}", header());
    consumers_marked(&hod(&sjlj), &[], "sjlj");
    // The coroutine seeds: their `.resume` and `.destroy` stubs are
    // function symbols, entered through the frame's pointers.
    let mut stubs = 0;
    for entry in
        std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/ir/coro")).unwrap()
    {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "ll") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let flag = "!9990 = !{i32 8, !\"cf-protection-branch\", i32 1}\n";
        let text = if text.contains("!llvm.module.flags = !{") {
            text.replacen("!llvm.module.flags = !{", "!llvm.module.flags = !{!9990, ", 1) + flag
        } else {
            text + "!llvm.module.flags = !{!9990}\n" + flag
        };
        let object = hod(&format!("{}{text}", header()));
        let name = path.display().to_string();
        consumers_marked(&object, &[], &name);
        let elf = Elf::parse(&object);
        stubs += elf.symbols.iter().filter(|s| s.name.ends_with(".resume")).count();
    }
    assert!(stubs > 0, "no coroutine stub was looked at");
}

// ---- the note ----

/// The sections of an ELF64 object by name: type, flags, alignment,
/// entry size and contents (`llc`'s objects name their sections from
/// `.strtab`, which the shared reader does not take).
fn sections(bytes: &[u8]) -> BTreeMap<String, (u32, u64, u64, u64, Vec<u8>)> {
    let u16_at = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let u64_at = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    let (shoff, count, names) = (u64_at(0x28) as usize, u16_at(0x3c), u16_at(0x3e));
    let header = |index: u16| shoff + 64 * usize::from(index);
    let names = header(names);
    let names = &bytes[u64_at(names + 24) as usize..][..u64_at(names + 32) as usize];
    let mut out = BTreeMap::new();
    for index in 1..count {
        let at = header(index);
        let name = &names[u32_at(at) as usize..];
        let name = String::from_utf8_lossy(&name[..name.iter().position(|&b| b == 0).unwrap()]);
        let kind = u32_at(at + 4);
        let (offset, size) = (u64_at(at + 24) as usize, u64_at(at + 32) as usize);
        let data = if kind == 8 { Vec::new() } else { bytes[offset..offset + size].to_vec() };
        out.insert(
            name.into_owned(),
            (kind, u64_at(at + 8), u64_at(at + 48), u64_at(at + 56), data),
        );
    }
    out
}

#[test]
fn the_property_note_is_llcs() {
    let dir = Scratch::new("x64-cet-note");
    for (flags, features) in [("", 0), (BRANCH, 1), (RETURN, 2), (FULL, 3)] {
        let text = seed(flags);
        let ours = sections(&hod(&text));
        let note = ours.get(".note.gnu.property");
        if features == 0 {
            assert!(note.is_none(), "a note without the flags");
        } else {
            let note = note.expect("the note");
            assert_eq!(note.4, elf::gnu_property_note(Arch::X86_64, features));
        }
        for major in MAJORS {
            let Some(object) = llc(&dir, major, "note", &text) else { continue };
            let theirs = sections(&std::fs::read(object).unwrap());
            assert_eq!(note, theirs.get(".note.gnu.property"), "llc {major}: {features}");
        }
    }
}

// ---- programs ----

/// A freestanding module for the programs: calls through pointers, an
/// `indirectbr`, a `returns_twice` callee; `@cmain` returns 42 when every
/// result is right, else the number of the first wrong one.
const PROGRAM: &str = "
@fp_tab = internal constant [2 x ptr] [ptr @l_tab, ptr @g_ext]
@ib_tab = internal constant [3 x ptr] [ptr blockaddress(@ib, %a), ptr blockaddress(@ib, %b), ptr blockaddress(@ib, %c)]
define internal i32 @l_tab(i32 %x) noinline {
  %r = add i32 %x, 1
  ret i32 %r
}
define i32 @g_ext(i32 %x) noinline {
  %r = add i32 %x, 40
  ret i32 %r
}
define i32 @rt(i32 %x) noinline returns_twice {
  ret i32 %x
}
define i32 @ib(i32 %x) noinline {
entry:
  %i = zext i32 %x to i64
  %p = getelementptr [3 x ptr], ptr @ib_tab, i64 0, i64 %i
  %t = load ptr, ptr %p
  %m = mul i32 %x, 100
  indirectbr ptr %t, [label %a, label %b, label %c]
a:
  %v = phi i32 [ %m, %entry ], [ %w, %c ]
  %r = add i32 %v, 1
  ret i32 %r
b:
  ret i32 7
c:
  %w = add i32 %m, 1000
  br label %a
}
define i32 @calls(i32 %x) noinline {
  %f0 = load ptr, ptr @fp_tab
  %a = call i32 %f0(i32 %x)
  %f1p = getelementptr [2 x ptr], ptr @fp_tab, i64 0, i64 1
  %f1 = load ptr, ptr %f1p
  %b = call i32 %f1(i32 %a)
  %c = call i32 @rt(i32 %b)
  ret i32 %c
}
";

/// `@cmain`: each `(call, expected)` in turn.
fn cmain(checks: &[(&str, i32)]) -> String {
    let mut text = String::from(
        "declare i32 @calls(i32)\ndeclare i32 @ib(i32)\ndeclare i32 @sj()\n\
         define i32 @cmain() {\nentry:\n  br label %k0\n",
    );
    for (index, (call, want)) in checks.iter().enumerate() {
        let n = index + 1;
        text += &format!(
            "k{index}:\n  %r{n} = call i32 {call}\n  %c{n} = icmp eq i32 %r{n}, {want}\n  \
             br i1 %c{n}, label %k{n}, label %bad{n}\nbad{n}:\n  ret i32 {n}\n"
        );
    }
    text + &format!("k{}:\n  ret i32 42\n}}\n", checks.len())
}

/// `_start`: `endbr64`, the stack aligned, `call cmain`, `exit` with
/// its result; with the property note `features`.
fn start(features: u32) -> Object {
    let mut obj = Object::new(Arch::X86_64);
    let text = obj.add_section(b".text", SectionKind::Text, 16).unwrap();
    let code = [
        0xf3, 0x0f, 0x1e, 0xfa, // endbr64
        0x31, 0xed, // xor ebp, ebp
        0x48, 0x83, 0xe4, 0xf0, // and rsp, -16
        0xe8, 0, 0, 0, 0, // call cmain
        0x89, 0xc7, // mov edi, eax
        0xb8, 60, 0, 0, 0, // mov eax, 60 (exit)
        0x0f, 0x05, // syscall
    ];
    objcommon::function_bytes(&mut obj, text, "_start", &code);
    let name = obj.add_string(b"cmain").unwrap();
    let cmain = obj.add_symbol(Symbol::undefined(name));
    obj.add_relocation(text, 11, X64Reloc::Plt32, cmain, -4);
    elf::add_gnu_property_note(&mut obj, features).unwrap();
    obj
}

/// Links `inputs` with every linker and runs the program: exit 42, and
/// the features `want` (and no other) in its notes.
fn link_and_check(tools: &Toolchain, dir: &Scratch, label: &str, inputs: &[&Path], want: &[&str]) {
    let Some(readelf) = tool("llvm-readelf") else { return };
    for linker in &tools.linkers {
        let mut flags = vec!["-nostdlib", "-static"];
        let shstk = want.contains(&"SHSTK");
        match linker.name {
            "bfd" if shstk => flags.push("-Wl,-z,cet-report=error"),
            "lld" if shstk => flags.extend(["-Wl,-z,force-ibt", "-Wl,-z,shstk"]),
            "lld" => flags.push("-Wl,-z,force-ibt"),
            _ => {}
        }
        let program = dir.join(format!("{label}-{}", linker.name));
        let ran = tools.link_and_run(linker, &flags, inputs, &program);
        assert_eq!(exit_code(&ran), Some(42), "{label}, {}", linker.name);
        let mut command = Command::new(&readelf);
        command.arg("--notes").arg(&program);
        let notes = run_with(&mut command, &[], Duration::from_mins(1)).stdout_text();
        let line = notes.lines().find(|l| l.contains("x86 feature")).unwrap_or("").to_owned();
        for feature in ["IBT", "SHSTK"] {
            assert_eq!(
                line.contains(feature),
                want.contains(&feature),
                "{label}, {}: {notes}",
                linker.name
            );
        }
    }
}

#[test]
fn programs_carry_ibt_and_shstk() {
    let dir = Scratch::new("x64-cet-programs");
    let Some(tools) = Toolchain::x86_64(&dir) else { return };
    let checks =
        [("@calls(i32 1)", 42), ("@ib(i32 0)", 1), ("@ib(i32 1)", 7), ("@ib(i32 2)", 1201)];
    let full =
        (format!("{}{PROGRAM}{FULL}", header()), format!("{}{}{FULL}", header(), cmain(&checks)));
    let mut sj_checks = checks.to_vec();
    sj_checks.push(("@sj()", 77));
    let branch = (
        format!("{}{PROGRAM}{SJLJ}{BRANCH}", header()),
        format!("{}{}{BRANCH}", header(), cmain(&sj_checks)),
    );
    for (label, (code, main), features, want) in
        [("full", full, 3, &["IBT", "SHSTK"][..]), ("branch", branch, 1, &["IBT"][..])]
    {
        let start = objcommon::linking::write(&dir, &format!("start-{label}.o"), &start(features));
        let code_hod = dir.write(&format!("{label}-code-hod.o"), hod(&code));
        let main_hod = dir.write(&format!("{label}-main-hod.o"), hod(&main));
        link_and_check(
            &tools,
            &dir,
            &format!("{label}-hod"),
            &[&start, &code_hod, &main_hod],
            want,
        );
        for major in MAJORS {
            let Some(code_llc) = llc(&dir, major, &format!("{label}-code"), &code) else {
                continue;
            };
            let Some(main_llc) = llc(&dir, major, &format!("{label}-main"), &main) else {
                continue;
            };
            let mixed = format!("{label}-hod-llc{major}");
            link_and_check(&tools, &dir, &mixed, &[&start, &code_hod, &main_llc], want);
            let mixed = format!("{label}-llc{major}-hod");
            link_and_check(&tools, &dir, &mixed, &[&start, &code_llc, &main_hod], want);
        }
    }
}

// ---- refusals and what needs no mark ----

#[test]
fn an_indirect_nocf_check_call_is_refused() {
    let callers = [
        "define i32 @f(ptr %p) {\n  %r = call i32 %p(i32 1) nocf_check\n  ret i32 %r\n}\n",
        "define i32 @f(ptr %p, i32 %x) {\n  %r = musttail call i32 %p(ptr %p, i32 %x) nocf_check\n  \
         ret i32 %r\n}\n",
    ];
    for caller in callers {
        let refused = try_hod(&format!("{}{caller}{BRANCH}", header()));
        let name = hod_x64::cet::NOCF_CHECK_INDIRECT_CALL;
        assert_eq!(refused, Err(CgError::unsupported(name)), "{caller}");
        // Without indirect-branch tracking the call is an ordinary one,
        // as in `llc` (no `notrack`).
        for flags in ["", RETURN] {
            assert!(try_hod(&format!("{}{caller}{flags}", header())).is_ok(), "{caller}");
        }
    }
    assert!(hod_x64::cet::UNSUPPORTED.contains(&hod_x64::cet::NOCF_CHECK_INDIRECT_CALL));
    // A direct one is a call.
    let direct = "declare i32 @g(i32)\ndefine i32 @f() {\n  %r = call i32 @g(i32 1) nocf_check\n  \
                  ret i32 %r\n}\n";
    assert!(try_hod(&format!("{}{direct}{BRANCH}", header())).is_ok());
}
