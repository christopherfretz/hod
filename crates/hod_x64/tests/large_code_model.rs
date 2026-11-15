//! The large code model on x86-64 (WP-209, `spec/codegen.md` §5.6.36):
//! one program (`tests/data/large/program.ll`) built by hod and by `llc`
//! 19 and 22, PIE and non-PIE. The large sections and the relocation types
//! of the code are `llc`'s; nothing but the thread pointer's offsets and
//! DWARF's section offsets is a 32-bit field. Linked with the C runtime's
//! start files under GNU ld, lld and mold, and without them in a layout
//! that puts the code, the GOT and the data more than 4 GiB apart (address
//! gaps, not file content), plain and with debug information, the program
//! prints what `llc`'s does.

#![allow(clippy::too_many_lines)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{Options, RelocModel};
use hod_ir::{Body, Module};
use hod_testutil::{Scratch, find_tool, run_with, tool};
use linking::{Linker, Toolchain};

/// What the program prints.
const EXPECTED: &str = "large code\n\ncatch=7 pick=13,-1 table=17 ops=5,8 same=1 tls=6 tail=42 \
                        div=100000000000000000 copy=101 big=77 ctor=1\ncounter=105\n";

/// 4 GiB: the distance the far layout puts between the code, the GOT and
/// the data.
const FOUR_GIB: u64 = 1 << 32;

/// A file of `tests/data/large`.
fn data(name: &str) -> String {
    let path = format!("{}/tests/data/large/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// How a module is built.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    /// PIC with `PIE Level` 2, linked `-pie`: what kiln gives by default.
    Pie,
    /// `-relocation-model=static`, linked `-no-pie`.
    Static,
}

impl Mode {
    const ALL: [Mode; 2] = [Mode::Pie, Mode::Static];

    fn name(self) -> &'static str {
        match self {
            Mode::Pie => "pie",
            Mode::Static => "static",
        }
    }

    fn llc_model(self) -> &'static str {
        match self {
            Mode::Pie => "-relocation-model=pic",
            Mode::Static => "-relocation-model=static",
        }
    }

    fn link_flag(self) -> &'static str {
        match self {
            Mode::Pie => "-pie",
            Mode::Static => "-no-pie",
        }
    }

    fn options(self) -> Options {
        match self {
            Mode::Pie => Options { reloc_model: RelocModel::Pic, ..Options::default() },
            Mode::Static => Options::default(),
        }
    }
}

/// `text` without its debug information: the metadata lines and every
/// `!dbg` attachment.
fn strip_debug(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        if line.starts_with('!') {
            continue;
        }
        let mut line = line.to_owned();
        while let Some(at) = line.find(" !dbg !") {
            let digits = line[at + 7..].bytes().take_while(u8::is_ascii_digit).count();
            let start = if line[..at].ends_with(',') { at - 1 } else { at };
            line.replace_range(start..at + 7 + digits, "");
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// The full text of a module of `tests/data/large`: the header, the body
/// (with or without its debug information) and the module flags.
fn module_text(body: &str, mode: Mode, debug: bool) -> String {
    let mut flags = vec!["!{i32 1, !\"Code Model\", i32 4}"];
    if mode == Mode::Pie {
        flags.push("!{i32 8, !\"PIC Level\", i32 2}");
        flags.push("!{i32 7, !\"PIE Level\", i32 2}");
    }
    let body = if debug {
        flags.push("!{i32 7, !\"Dwarf Version\", i32 5}");
        flags.push("!{i32 2, !\"Debug Info Version\", i32 3}");
        body.to_owned()
    } else {
        strip_debug(body)
    };
    let mut text = format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n{body}",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    );
    let ids: Vec<String> = (0..flags.len()).map(|i| format!("!{}", 900 + i)).collect();
    let _ = writeln!(text, "!llvm.module.flags = !{{{}}}", ids.join(", "));
    for (id, flag) in ids.iter().zip(&flags) {
        let _ = writeln!(text, "{id} = {flag}");
    }
    text
}

/// A module and its bodies from `.ll` text.
fn parse(text: &str) -> (Module, Vec<Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, parsed| {
        let body =
            parsed.body.unwrap_or_else(|| panic!("hod_ll: unsupported {:?}", parsed.unsupported));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    (module, bodies)
}

/// Who builds an object: hod, or `llc` of one LLVM major.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Who {
    Hod,
    Llc(u32),
}

impl Who {
    fn name(self) -> String {
        match self {
            Who::Hod => "hod".to_owned(),
            Who::Llc(major) => format!("llc{major}"),
        }
    }
}

/// The object of `text`; `None` when that `llc` is not installed.
fn object(dir: &Scratch, name: &str, text: &str, mode: Mode, who: Who) -> Option<PathBuf> {
    let name = format!("{name}.{}.{}", mode.name(), who.name());
    match who {
        Who::Hod => {
            let (module, bodies) = parse(text);
            let bytes = hod_x64::compile_to_object(&module, &bodies, &mode.options())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            Some(dir.write(&format!("{name}.o"), bytes))
        }
        Who::Llc(major) => {
            let llc = tool(&format!("llc-{major}"))?;
            let input = dir.write(&format!("{name}.ll"), text);
            let output = dir.join(format!("{name}.o"));
            let mut command = Command::new(llc);
            command
                .args(["-O0", "-filetype=obj", mode.llc_model()])
                .arg(&input)
                .arg("-o")
                .arg(&output);
            let ran = run_with(&mut command, &[], Duration::from_mins(2));
            assert!(ran.success(), "llc-{major} {name}: {}", ran.stderr_text());
            Some(output)
        }
    }
}

/// Runs a tool, requiring success; its standard output.
fn output(program: &Path, args: &[&OsStr]) -> String {
    let mut command = Command::new(program);
    command.args(args);
    let ran = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(ran.success(), "{}: {}", program.display(), ran.stderr_text());
    ran.stdout_text()
}

/// A section as `llvm-readelf -S` prints it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Section {
    name: String,
    address: u64,
    flags: String,
}

/// The sections of an ELF file, by name (the first of a name).
fn sections(file: &Path) -> Option<BTreeMap<String, Section>> {
    let readelf = tool("llvm-readelf")?;
    let text = output(&readelf, &["-SW".as_ref(), file.as_os_str()]);
    let mut out = BTreeMap::new();
    for line in text.lines() {
        // `[Nr] Name Type Address Off Size ES Flg Lk Inf Al`: the flags
        // are absent for a section without any.
        let Some((_, rest)) = line.split_once(']') else { continue };
        let fields: Vec<&str> = rest.split_whitespace().collect();
        if fields.len() < 9 || fields[0] == "Name" {
            continue;
        }
        let Ok(address) = u64::from_str_radix(fields[2], 16) else { continue };
        let flags = if fields.len() == 10 { fields[6].to_owned() } else { String::new() };
        let section = Section { name: fields[0].to_owned(), address, flags };
        out.entry(section.name.clone()).or_insert(section);
    }
    Some(out)
}

/// The relocation types of an object, by the section they apply to.
fn relocation_types(object: &Path) -> Option<BTreeMap<String, BTreeSet<String>>> {
    let readelf = tool("llvm-readelf")?;
    let text = output(&readelf, &["-r".as_ref(), object.as_os_str()]);
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut section = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Relocation section '.rela") {
            rest.split('\'').next().unwrap_or_default().clone_into(&mut section);
            continue;
        }
        if let Some(kind) = line.split_whitespace().nth(2).filter(|k| k.starts_with("R_X86_64_")) {
            out.entry(section.clone()).or_default().insert(kind.to_owned());
        }
    }
    Some(out)
}

/// The large sections (`SHF_X86_64_LARGE`, flag `l`) of an object, with
/// their flags.
fn large_sections(object: &Path) -> Option<BTreeSet<(String, String)>> {
    let found = sections(object)?;
    Some(found.into_values().filter(|s| s.flags.contains('l')).map(|s| (s.name, s.flags)).collect())
}

/// The sections and relocations of hod's object are `llc`'s: the same
/// large sections with the same flags, thread-local sections never large,
/// the same relocation types in the code, and no 32-bit field but the
/// thread pointer's offsets and DWARF's offsets into its own sections.
/// (hod computes the GOT's address at each reference, so it has more
/// `R_X86_64_GOTPC64`; `llc`'s jump tables are `.rodata`, which hod does
/// not have; `llc` writes absolute pointers in the exception tables
/// outside PIC, hod PC-relative ones in every model.)
#[test]
fn sections_and_relocations_are_llcs() {
    let dir = Scratch::new("large-objects");
    let program = data("program.ll");
    for mode in Mode::ALL {
        let text = module_text(&program, mode, true);
        let hod = object(&dir, "program", &text, mode, Who::Hod).unwrap();
        let Some(hod_large) = large_sections(&hod) else { return };
        let hod_relocs = relocation_types(&hod).unwrap();
        let hod_sections = sections(&hod).unwrap();
        for section in [".tdata", ".tbss", ".init_array"] {
            let flags = &hod_sections[section].flags;
            assert!(!flags.contains('l'), "{mode:?}: {section} is large: {flags}");
        }
        let code = &hod_relocs[".ltext"];
        let expected: &[&str] = match mode {
            Mode::Pie => {
                &["R_X86_64_GOT64", "R_X86_64_GOTOFF64", "R_X86_64_GOTPC64", "R_X86_64_TPOFF32"]
            }
            Mode::Static => &["R_X86_64_64", "R_X86_64_TPOFF32"],
        };
        assert_eq!(code.iter().map(String::as_str).collect::<Vec<_>>(), expected, "{mode:?}");
        for (section, kinds) in &hod_relocs {
            for kind in kinds {
                let wide = kind.ends_with("64");
                let allowed = wide
                    || kind == "R_X86_64_TPOFF32" && section == ".ltext"
                    || kind == "R_X86_64_32" && section.starts_with(".debug");
                assert!(allowed, "{mode:?}: {kind} in {section}");
            }
        }
        for major in [19, 22] {
            let Some(llc) = object(&dir, "program", &text, mode, Who::Llc(major)) else {
                continue;
            };
            let llc_large = large_sections(&llc).unwrap();
            assert_eq!(hod_large, llc_large, "{mode:?}: llc-{major}'s large sections");
            let llc_relocs = relocation_types(&llc).unwrap();
            assert_eq!(code, &llc_relocs[".ltext"], "{mode:?}: llc-{major}'s code relocations");
        }
    }
}

/// The C program that enters at `_start` without the C runtime's start
/// files and calls `start_c` (`tests/data/large/start.ll`), the stack
/// aligned. Its call is to code within 2 GiB.
const START_STUB: &str = concat!(
    "__asm__(\".text\\n.globl _start\\n.type _start,@function\\n_start:\\n",
    " xorl %ebp, %ebp\\n movq (%rsp), %rdi\\n leaq 8(%rsp), %rsi\\n movq %rsp, %rcx\\n",
    " andq $-16, %rsp\\n call start_c\\n hlt\\n.size _start, .-_start\\n\");\n"
);

/// One linker of the matrix, with the version told apart.
struct Link {
    label: String,
    linker: Linker,
}

/// GNU ld, the newest lld, lld 19 when installed, and mold.
fn linkers(scratch: &Scratch, toolchain: &Toolchain) -> Vec<Link> {
    let mut out: Vec<Link> = toolchain
        .linkers
        .iter()
        .map(|linker| Link { label: linker.name.to_owned(), linker: linker.clone() })
        .collect();
    if let Some(linker) = Toolchain::lld(scratch, 19) {
        out.push(Link { label: "lld19".to_owned(), linker });
    }
    out
}

/// The far layout for one linker: `--section-start` of the sections that
/// make code, GOT and data at least 4 GiB apart, as each linker lays out
/// the rest. GNU ld and mold place a section after its usual predecessor;
/// outside PIC the GOT stays with the PLT, whose stubs reach it with
/// 32-bit displacements, and mold keeps `.lrodata` with the read-only data
/// before the code. lld places what is not given after the highest
/// address given (the code is last, so the frame index, whose table is
/// 32-bit, is near it), puts sections of the same permissions that are
/// adjacent in address order in one segment (so a read-only, a writable
/// and again a read-only section alternate, and only `.lbss` follows
/// `.ldata`), and is given the C runtime's `.text` just before `.ltext`
/// (with `.ltext` first, its frame index is unsorted:
/// [`lld_22_sorts_the_frame_index`]).
fn far_flags(label: &str, mode: Mode) -> Vec<String> {
    let mut starts: Vec<(&str, u64)> = match label {
        "bfd" => {
            vec![(".lrodata", 0x4_0000_0000), (".ldata", 0x6_0000_0000), (".lbss", 0x8_0000_0000)]
        }
        "mold" => vec![(".ldata", 0x6_0000_0000), (".lbss", 0x8_0000_0000)],
        _ => vec![
            (".ldata", 0x2_0000_0000),
            (".lbss", 0x4_0000_0000),
            (".lrodata", 0x6_0000_0000),
            (".text", 0xa_0000_0000),
            (".ltext", 0xa_0001_0000),
        ],
    };
    if mode == Mode::Pie {
        // GNU ld and mold place the GOT after `.init_array`.
        starts.push(if label.starts_with("lld") {
            (".got", 0x8_0000_0000)
        } else {
            (".init_array", 0x2_0000_0000)
        });
    }
    let mut flags: Vec<String> =
        starts.iter().map(|(name, at)| format!("-Wl,--section-start={name}={at:#x}")).collect();
    if label.starts_with("lld") || label == "mold" && mode == Mode::Pie {
        // lld keeps the RELRO sections together, and the GOT apart from
        // `.init_array` splits them. mold keeps `.tdata` and `.dynamic`
        // low and RELRO through the far `.init_array` and `.got`: its
        // `PT_GNU_RELRO` spans 8 GiB of unmapped gap, and glibc's loader
        // refuses it natively ("cannot apply additional memory protection
        // after relocation: Cannot allocate memory"; qemu-user does not),
        // which `linking::check_loadable` asserts. GNU ld puts `.tdata`
        // and `.dynamic` after `.init_array`, so its RELRO is contiguous.
        flags.push("-Wl,-z,norelro".to_owned());
    }
    flags
}

/// The distance between two addresses.
fn apart(a: u64, b: u64) -> u64 {
    a.abs_diff(b)
}

/// Asserts the far layout of a linked program: code, GOT and the large
/// data each more than 4 GiB from the others.
fn check_far_layout(program: &Path, label: &str, mode: Mode) {
    let Some(found) = sections(program) else { return };
    let at =
        |name: &str| found.get(name).map_or_else(|| panic!("{label}: no {name}"), |s| s.address);
    let code = at(".ltext");
    let mut far = vec![(".ldata", at(".ldata")), (".lbss", at(".lbss"))];
    if label != "mold" {
        far.push((".lrodata", at(".lrodata")));
    }
    if mode == Mode::Pie {
        far.push((".got", at(".got")));
    }
    for (i, (name, address)) in far.iter().enumerate() {
        let distance = apart(code, *address);
        assert!(distance > FOUR_GIB, "{label} {mode:?}: .ltext and {name} are {distance:#x} apart");
        for (other, other_address) in &far[i + 1..] {
            let distance = apart(*address, *other_address);
            assert!(distance > FOUR_GIB, "{label} {mode:?}: {name} and {other} {distance:#x}");
        }
    }
}

/// Links `inputs` and the C++ runtime, runs the program and compares
/// what it prints.
fn link_and_check(
    toolchain: &Toolchain,
    link: &Link,
    flags: &[&str],
    inputs: &[&Path],
    output: &Path,
) {
    let mut inputs = inputs.to_vec();
    inputs.push(Path::new("-lstdc++"));
    let ran = toolchain.link_and_run(&link.linker, flags, &inputs, output);
    linking::check_loadable(output, toolchain.target);
    assert!(
        ran.success(),
        "{} {}: {:?}\n{}",
        link.label,
        output.display(),
        ran.status,
        ran.stderr_text()
    );
    assert_eq!(ran.stdout_text(), EXPECTED, "{} {}", link.label, output.display());
}

/// Whether lld's index of the frames is wrong for this layout: lld 22
/// writes `.eh_frame_hdr`'s table unsorted when `.ltext` comes before the
/// C runtime's `.text` (see [`lld_22_sorts_the_frame_index`]).
fn lld_22_with_start_files(link: &Link) -> bool {
    link.label == "lld"
}

/// The program linked with the C runtime's start files, as a compiler
/// driver links it, plain and with debug information: what `llc`'s
/// objects print, under every linker but the newest lld.
#[test]
fn runs_linked_with_the_start_files() {
    let scratch = Scratch::new("large-crt");
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let linkers = linkers(&scratch, &toolchain);
    let program = data("program.ll");
    for mode in Mode::ALL {
        for debug in [false, true] {
            let text = module_text(&program, mode, debug);
            let tag = if debug { "g" } else { "plain" };
            for who in [Who::Hod, Who::Llc(19), Who::Llc(22)] {
                let Some(object) = object(&scratch, &format!("crt-{tag}"), &text, mode, who) else {
                    continue;
                };
                for link in &linkers {
                    if lld_22_with_start_files(link) {
                        continue;
                    }
                    let output = scratch.join(format!(
                        "crt-{tag}.{}.{}.{}",
                        mode.name(),
                        who.name(),
                        link.label
                    ));
                    link_and_check(&toolchain, link, &[mode.link_flag()], &[&object], &output);
                }
            }
        }
    }
}

/// The program and `start.ll` linked without the C runtime's start files,
/// the code, the GOT and the large data more than 4 GiB apart: under
/// every linker, hod's objects and `llc`'s print the same, the debug
/// information verifies and names addresses beyond 4 GiB.
#[test]
fn runs_with_code_got_and_data_far_apart() {
    let scratch = Scratch::new("large-far");
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let linkers = linkers(&scratch, &toolchain);
    let stub = toolchain.compile_c(&scratch, "stub", START_STUB, &[]);
    let program = data("program.ll");
    let start = data("start.ll");
    let dwarfdump = find_tool("llvm-dwarfdump");
    for mode in Mode::ALL {
        let start_text = module_text(&start, mode, false);
        let start = object(&scratch, "start", &start_text, mode, Who::Hod).unwrap();
        for debug in [false, true] {
            let text = module_text(&program, mode, debug);
            let tag = if debug { "g" } else { "plain" };
            for who in [Who::Hod, Who::Llc(19), Who::Llc(22)] {
                let Some(object) = object(&scratch, &format!("far-{tag}"), &text, mode, who) else {
                    continue;
                };
                for link in &linkers {
                    // wild leaves the GOT, `.init_array` and the frame
                    // index where it lays them out and puts `.ltext` after
                    // the large data: no layout of `--section-start`s
                    // gives it the far layout (case `section-start`). The
                    // programs linked with the start files still run.
                    if hod_testutil::linkers::exception(&link.label, "section-start").is_some() {
                        continue;
                    }
                    let output = scratch.join(format!(
                        "far-{tag}.{}.{}.{}",
                        mode.name(),
                        who.name(),
                        link.label
                    ));
                    let far = far_flags(&link.label, mode);
                    let mut flags = vec![mode.link_flag(), "-nostartfiles"];
                    flags.extend(far.iter().map(String::as_str));
                    let inputs = [stub.as_path(), start.as_path(), object.as_path()];
                    link_and_check(&toolchain, link, &flags, &inputs, &output);
                    check_far_layout(&output, &link.label, mode);
                    let size = std::fs::metadata(&output).unwrap().len();
                    assert!(size < 1 << 20, "{}: {size} bytes: gaps became content", link.label);
                    if debug
                        && who == Who::Hod
                        && let Some(dwarfdump) = &dwarfdump
                    {
                        let verify =
                            output_of(dwarfdump, &["--verify".as_ref(), output.as_os_str()]);
                        assert!(verify.contains("No errors."), "{}: {verify}", link.label);
                        let lines =
                            output_of(dwarfdump, &["--debug-line".as_ref(), output.as_os_str()]);
                        let code = sections(&output).unwrap()[".ltext"].address;
                        let rows = line_addresses(&lines);
                        assert!(!rows.is_empty(), "{}: no line rows", link.label);
                        for row in rows {
                            assert!(row >= code, "{}: a line row at {row:#x}", link.label);
                        }
                    }
                    std::fs::remove_file(&output).unwrap();
                }
            }
        }
    }
}

/// A tool's standard output, whatever its status.
fn output_of(program: &Path, args: &[&OsStr]) -> String {
    let mut command = Command::new(program);
    command.args(args);
    let ran = run_with(&mut command, &[], Duration::from_mins(1));
    ran.stdout_text()
}

/// The addresses of the rows of `llvm-dwarfdump --debug-line`'s tables.
fn line_addresses(text: &str) -> Vec<u64> {
    text.lines()
        .filter_map(|line| line.strip_prefix("0x"))
        .filter_map(|line| u64::from_str_radix(line.split_whitespace().next()?, 16).ok())
        .collect()
}

/// lld 22.1.8 sorts `.eh_frame_hdr`'s binary-search table out of address
/// order when an object's `.ltext` is placed before the C runtime's
/// `.text` (the start files' frame first: initial locations 0x3b30, 0x18d0,
/// 0x1950 for `_start`, `thrower`, `main`), so the unwinder finds no frame
/// for the large code and the exception terminates the program. `llc`'s
/// own objects fail the same way; GNU ld and mold sort the table, lld 19
/// does here but not when `.ltext` is placed above the start stub's
/// `.text` with `--section-start` (hence the far layout's order). Not
/// hod's: an lld defect, kept as a failing test until lld fixes it.
#[test]
#[ignore = "lld 22 writes .eh_frame_hdr unsorted with .ltext before .text (an lld defect)"]
fn lld_22_sorts_the_frame_index() {
    let scratch = Scratch::new("large-lld22");
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let text = module_text(&data("program.ll"), Mode::Pie, false);
    let Some(object) = object(&scratch, "lld22", &text, Mode::Pie, Who::Llc(22)) else { return };
    let Some(lld) = toolchain.linkers.iter().find(|l| l.name == "lld") else { return };
    let link = Link { label: "lld".to_owned(), linker: lld.clone() };
    let output = scratch.join("lld22.pie");
    link_and_check(&toolchain, &link, &["-pie"], &[&object], &output);
}

/// A call of an `available_externally` `alwaysinline` function goes to
/// its local copy under the large code model as under the small one
/// (`spec/codegen.md` §9.7.2): the `movabs` of the callee names the copy,
/// and the external symbol is left to the address taken.
#[test]
fn calls_go_to_local_copies() {
    let body = "define available_externally i32 @helper(i32 %x) alwaysinline {\n\
                \x20 %r = add i32 %x, 1\n  ret i32 %r\n}\n\
                define i32 @user(i32 %x) {\n  %r = call i32 @helper(i32 %x)\n  ret i32 %r\n}\n\
                define ptr @address() {\n  ret ptr @helper\n}\n";
    let dir = Scratch::new("large-copies");
    for mode in Mode::ALL {
        let text = module_text(body, mode, false);
        let object = object(&dir, "copies", &text, mode, Who::Hod).unwrap();
        let Some(readelf) = tool("llvm-readelf") else { return };
        let relocs = output(&readelf, &["-r".as_ref(), object.as_os_str()]);
        let to = |name: &str| {
            relocs
                .lines()
                .filter(|line| line.split_whitespace().nth(4) == Some(name))
                .map(|line| line.split_whitespace().nth(2).unwrap_or_default().to_owned())
                .collect::<Vec<_>>()
        };
        let (copy, external) = match mode {
            Mode::Pie => (vec!["R_X86_64_GOT64"], vec!["R_X86_64_GOT64"]),
            Mode::Static => (vec!["R_X86_64_64"], vec!["R_X86_64_64"]),
        };
        assert_eq!(to("helper.alwaysinline"), copy, "{mode:?}:\n{relocs}");
        assert_eq!(to("helper"), external, "{mode:?}:\n{relocs}");
    }
}
