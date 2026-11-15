//! The large code model on AArch64 (WP-209, `spec/codegen.md` §5.6.36):
//! the x86-64 test's program (`hod_x64/tests/data/large/program.ll`) built
//! by hod and by `llc` 19 and 22 outside PIC. The relocation types of the
//! code and of the unwind tables are `llc`'s: a symbol's address is four
//! `movz`/`movk` (`R_AARCH64_MOVW_UABS_G0_NC` to `G3`), a symbol that may
//! be preemptible is read from the GOT (`adrp`, which `llc` keeps), calls
//! stay `bl`, and the frames' and exception tables' pointers are 8 bytes.
//! Linked with the C runtime's start files under GNU ld, lld and mold,
//! and without them in a layout that puts the code more than 4 GiB from
//! the data (address gaps, not file content), plain and with debug
//! information, the program prints what `llc`'s does. Under PIC the model
//! is refused by name.

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

use hod_cg::{CgError, Options, RelocModel};
use hod_ir::{Body, Module};
use hod_testutil::{Scratch, find_tool, run_with, tool};
use linking::{Linker, Toolchain};

/// What the program prints.
const EXPECTED: &str = "large code\n\ncatch=7 pick=13,-1 table=17 ops=5,8 same=1 tls=6 tail=42 \
                        div=100000000000000000 copy=101 big=77 ctor=1\ncounter=105\n";

/// 4 GiB: the distance the far layout puts between the code and the data.
const FOUR_GIB: u64 = 1 << 32;

/// A file of the x86-64 test's `tests/data/large`: the program is the
/// same on both targets.
fn data(name: &str) -> String {
    let path = format!("{}/../hod_x64/tests/data/large/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
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

/// The full text of a module: the header, the body (with or without its
/// debug information) and the module flags, the large code model's among
/// them.
fn module_text(body: &str, debug: bool) -> String {
    let mut flags = vec!["!{i32 1, !\"Code Model\", i32 4}"];
    let body = if debug {
        flags.push("!{i32 7, !\"Dwarf Version\", i32 5}");
        flags.push("!{i32 2, !\"Debug Info Version\", i32 3}");
        body.to_owned()
    } else {
        strip_debug(body)
    };
    let mut text = format!(
        "target datalayout = \"{}\"\ntarget triple = \"aarch64-unknown-linux-gnu\"\n{body}",
        hod_ir::AARCH64_LINUX_DATA_LAYOUT
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

/// The static object of `text`; `None` when that `llc` is not installed.
fn object(dir: &Scratch, name: &str, text: &str, who: Who) -> Option<PathBuf> {
    let name = format!("{name}.{}", who.name());
    match who {
        Who::Hod => {
            let (module, bodies) = parse(text);
            let bytes = hod_a64::compile_to_object(&module, &bodies, &Options::default())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            Some(dir.write(&format!("{name}.o"), bytes))
        }
        Who::Llc(major) => {
            let llc = tool(&format!("llc-{major}"))?;
            let input = dir.write(&format!("{name}.ll"), text);
            let output = dir.join(format!("{name}.o"));
            let mut command = Command::new(llc);
            command
                .args(["-O0", "-filetype=obj", "-relocation-model=static"])
                .arg(&input)
                .arg("-o")
                .arg(&output);
            let ran = run_with(&mut command, &[], Duration::from_mins(2));
            assert!(ran.success(), "llc-{major} {name}: {}", ran.stderr_text());
            Some(output)
        }
    }
}

/// A tool's standard output, requiring success.
fn output(program: &Path, args: &[&OsStr]) -> String {
    let mut command = Command::new(program);
    command.args(args);
    let ran = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(ran.success(), "{}: {}", program.display(), ran.stderr_text());
    ran.stdout_text()
}

/// A tool's standard output, whatever its status.
fn output_of(program: &Path, args: &[&OsStr]) -> String {
    let mut command = Command::new(program);
    command.args(args);
    let ran = run_with(&mut command, &[], Duration::from_mins(1));
    ran.stdout_text()
}

/// The addresses of an ELF file's sections, by name (the first of a name).
fn addresses(file: &Path) -> Option<BTreeMap<String, u64>> {
    let readelf = tool("llvm-readelf")?;
    let text = output(&readelf, &["-SW".as_ref(), file.as_os_str()]);
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Some((_, rest)) = line.split_once(']') else { continue };
        let fields: Vec<&str> = rest.split_whitespace().collect();
        if fields.len() < 9 || fields[0] == "Name" {
            continue;
        }
        let Ok(address) = u64::from_str_radix(fields[2], 16) else { continue };
        out.entry(fields[0].to_owned()).or_insert(address);
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
        if let Some(kind) = line.split_whitespace().nth(2).filter(|k| k.starts_with("R_AARCH64_")) {
            out.entry(section.clone()).or_default().insert(kind.to_owned());
        }
    }
    Some(out)
}

/// The relocation types of the code and of the unwind tables are
/// `llc`'s, and none is a 32-bit or page-relative reach to data: the
/// addresses are `movz`/`movk`, the GOT's `adrp` is the one `llc` keeps,
/// and the frames' and exception tables' pointers are 8 bytes.
#[test]
fn relocations_are_llcs() {
    let dir = Scratch::new("a64-large-objects");
    let text = module_text(&data("program.ll"), true);
    let hod = object(&dir, "program", &text, Who::Hod).unwrap();
    let Some(hod_relocs) = relocation_types(&hod) else { return };
    let code: Vec<&str> = hod_relocs[".text"].iter().map(String::as_str).collect();
    assert_eq!(
        code,
        [
            "R_AARCH64_ADR_GOT_PAGE",
            "R_AARCH64_CALL26",
            "R_AARCH64_JUMP26",
            "R_AARCH64_LD64_GOT_LO12_NC",
            "R_AARCH64_MOVW_UABS_G0_NC",
            "R_AARCH64_MOVW_UABS_G1_NC",
            "R_AARCH64_MOVW_UABS_G2_NC",
            "R_AARCH64_MOVW_UABS_G3",
            "R_AARCH64_TLSLE_ADD_TPREL_HI12",
            "R_AARCH64_TLSLE_ADD_TPREL_LO12_NC",
        ]
    );
    for section in [".eh_frame", ".gcc_except_table"] {
        let kinds: Vec<&str> = hod_relocs[section].iter().map(String::as_str).collect();
        assert_eq!(kinds, ["R_AARCH64_PREL64"], "{section}");
    }
    for major in [19, 22] {
        let Some(llc) = object(&dir, "program", &text, Who::Llc(major)) else { continue };
        let llc_relocs = relocation_types(&llc).unwrap();
        for section in [".text", ".eh_frame", ".gcc_except_table"] {
            assert_eq!(hod_relocs[section], llc_relocs[section], "llc-{major}: {section}");
        }
    }
}

/// Under PIC, `llc`'s large model is neither small PIC's code (its unwind
/// tables are 8-byte) nor complete (it rejects thread-locals other than
/// local exec): hod refuses it by name.
#[test]
fn pic_is_refused_by_name() {
    let (module, bodies) = parse(&module_text(&data("program.ll"), false));
    let options = Options { reloc_model: RelocModel::Pic, ..Options::default() };
    match hod_a64::compile_to_object(&module, &bodies, &options) {
        Err(hod_a64::ModuleError::Function { error, .. }) => {
            assert_eq!(error, CgError::unsupported(hod_cg::MODULE_CODE_MODEL_LARGE_PIC));
        }
        other => panic!("{:?}", other.map(|_| ())),
    }
}

/// The C program that enters at `_start` without the C runtime's start
/// files and calls `start_c` (`start.ll`) with the dynamic linker's
/// finalizer and the stack's end, as `__libc_start_main` takes them.
const START_STUB: &str = concat!(
    "__asm__(\".text\\n.globl _start\\n.type _start,%function\\n_start:\\n",
    " mov x29, #0\\n mov x30, #0\\n mov x2, x0\\n ldr x0, [sp]\\n add x1, sp, #8\\n",
    " mov x3, sp\\n bl start_c\\n brk #0\\n.size _start, .-_start\\n\");\n"
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
/// put the code more than 4 GiB from `.data`, `.bss` and (but under mold)
/// `.rodata`. The GOT stays within 4 GiB of the code: `llc`'s large model
/// reads it with `adrp`, as the PLT reads `.got.plt`. GNU ld keeps the
/// frame index with the code (its table is 32-bit). lld places what is
/// not given after the highest address given, and keeps the first
/// read-only sections with the ELF header: `.rodata` goes last, the code,
/// the frame index, the PLT and the GOT before it (so the exception
/// tables, which follow `.rodata`, are far from the code too), the RELRO
/// sections apart. mold keeps `.rodata` before the code (anchoring it
/// moves the code), and is kept from relaxing a GOT load into an `adrp`
/// of a symbol out of its reach.
fn far_flags(label: &str) -> Vec<String> {
    let mut starts: Vec<(&str, u64)> = vec![(".data", 0x4_0000_0000), (".bss", 0x6_0000_0000)];
    let mut flags = Vec::new();
    match label {
        "bfd" => {
            starts.push((".rodata", 0x2_0000_0000));
            starts.push((".eh_frame_hdr", 0x1000_0000));
        }
        "mold" => {
            starts.push((".got.plt", 0x2000_0000));
            flags.push("-Wl,--no-relax".to_owned());
        }
        // wild keeps `.rodata` and the PLT where it lays them out whatever
        // `--section-start` says (`hod_testutil::linkers::EXCEPTIONS`,
        // case `section-start`): the code stays with them, the data alone
        // is moved.
        "wild" => {}
        _ => {
            starts.extend([
                (".text", 0x8_0000_0000),
                (".eh_frame_hdr", 0x8_1000_0000),
                (".eh_frame", 0x8_1001_0000),
                (".plt", 0x8_2000_0000),
                (".got", 0x8_3000_0000),
                (".got.plt", 0x8_3001_0000),
                (".rodata", 0xa_0000_0000),
            ]);
            flags.push("-Wl,-z,norelro".to_owned());
        }
    }
    flags.extend(starts.iter().map(|(name, at)| format!("-Wl,--section-start={name}={at:#x}")));
    flags
}

/// Asserts the far layout: the code more than 4 GiB from the data.
fn check_far_layout(program: &Path, label: &str) {
    let Some(found) = addresses(program) else { return };
    let at = |name: &str| *found.get(name).unwrap_or_else(|| panic!("{label}: no {name}"));
    let code = at(".text");
    let mut far = vec![".data", ".bss"];
    let rodata_moves =
        label != "mold" && hod_testutil::linkers::exception(label, "section-start").is_none();
    if rodata_moves {
        far.push(".rodata");
    }
    for name in far {
        let distance = code.abs_diff(at(name));
        assert!(distance > FOUR_GIB, "{label}: .text and {name} are {distance:#x} apart");
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

/// The program linked with the C runtime's start files, plain and with
/// debug information: what `llc`'s objects print, under every linker.
#[test]
fn runs_linked_with_the_start_files() {
    let scratch = Scratch::new("a64-large-crt");
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    let linkers = linkers(&scratch, &toolchain);
    let program = data("program.ll");
    for debug in [false, true] {
        let text = module_text(&program, debug);
        let tag = if debug { "g" } else { "plain" };
        for who in [Who::Hod, Who::Llc(19), Who::Llc(22)] {
            let Some(object) = object(&scratch, &format!("crt-{tag}"), &text, who) else {
                continue;
            };
            for link in &linkers {
                let output = scratch.join(format!("crt-{tag}.{}.{}", who.name(), link.label));
                link_and_check(&toolchain, link, &["-no-pie"], &[&object], &output);
            }
        }
    }
}

/// The addresses of the rows of `llvm-dwarfdump --debug-line`'s tables.
fn line_addresses(text: &str) -> Vec<u64> {
    text.lines()
        .filter_map(|line| line.strip_prefix("0x"))
        .filter_map(|line| u64::from_str_radix(line.split_whitespace().next()?, 16).ok())
        .collect()
}

/// The program and `start.ll` linked without the C runtime's start files,
/// the code more than 4 GiB from the data: under every linker, hod's
/// objects and `llc`'s print the same, and the debug information verifies.
#[test]
fn runs_with_code_and_data_far_apart() {
    let scratch = Scratch::new("a64-large-far");
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    let linkers = linkers(&scratch, &toolchain);
    let stub = toolchain.compile_c(&scratch, "stub", START_STUB, &[]);
    let start =
        object(&scratch, "start", &module_text(&data("start.ll"), false), Who::Hod).unwrap();
    let program = data("program.ll");
    let dwarfdump = find_tool("llvm-dwarfdump");
    for debug in [false, true] {
        let text = module_text(&program, debug);
        let tag = if debug { "g" } else { "plain" };
        for who in [Who::Hod, Who::Llc(19), Who::Llc(22)] {
            let Some(object) = object(&scratch, &format!("far-{tag}"), &text, who) else {
                continue;
            };
            for link in &linkers {
                let output = scratch.join(format!("far-{tag}.{}.{}", who.name(), link.label));
                let far = far_flags(&link.label);
                let mut flags = vec!["-no-pie", "-nostartfiles"];
                flags.extend(far.iter().map(String::as_str));
                let inputs = [stub.as_path(), start.as_path(), object.as_path()];
                link_and_check(&toolchain, link, &flags, &inputs, &output);
                check_far_layout(&output, &link.label);
                let size = std::fs::metadata(&output).unwrap().len();
                assert!(size < 1 << 20, "{}: {size} bytes: gaps became content", link.label);
                if debug
                    && who == Who::Hod
                    && let Some(dwarfdump) = &dwarfdump
                {
                    let verify = output_of(dwarfdump, &["--verify".as_ref(), output.as_os_str()]);
                    assert!(verify.contains("No errors."), "{}: {verify}", link.label);
                    let lines =
                        output_of(dwarfdump, &["--debug-line".as_ref(), output.as_os_str()]);
                    let code = addresses(&output).unwrap()[".text"];
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
