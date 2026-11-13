//! Compressed debug sections end to end (`spec/object.md` "Compressed
//! sections", `spec/dwarf.md` §1.1, WP-061). Each case's `-g` IR (clang
//! 19 and 22) is compiled by hod twice, plain and with
//! `debug_compression: Zlib`, and the compressed object must be the plain
//! one with its `.debug_*` sections compressed:
//!
//! - every section of the plain object is in the compressed one, in the
//!   same order; a `.debug_*` section either has `SHF_COMPRESSED`, an
//!   `Elf64_Chdr` (zlib, the plain size and alignment), alignment 8 and a
//!   stream that Python's `zlib` and `llvm-objcopy
//!   --decompress-debug-sections` decode to the plain bytes and that is
//!   smaller than them, or is the plain section unchanged; every other
//!   section (relocations included) is unchanged;
//! - `llvm-dwarfdump --verify` (19 and 22) is clean, and `llvm-dwarfdump
//!   --all` and GNU `readelf --debug-dump` print what they print for the
//!   plain object, with no complaint;
//! - gdb and lldb, on the object itself, answer as for the plain one;
//! - linked by GNU ld, lld and mold, the program runs, its debug
//!   information reads (`llvm-dwarfdump`) as that of the plain object's
//!   program, and gdb and lldb sessions print the same: the relocations
//!   against the compressed sections were applied.

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use common::parse;
use hod_cg::Options;
use hod_obj::Compression;
use hod_testutil::{Scratch, run_with, tool};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn output(command: &mut Command) -> String {
    let out = run_with(command, &[], Duration::from_mins(3));
    assert!(out.success(), "{command:?}: {}{}", out.stdout_text(), out.stderr_text());
    out.stdout_text()
}

/// `-g -O0` IR of `source` by `clang`, with `flag` added.
fn ir(clang: &Path, scratch: &Scratch, name: &str, source: &Path, flag: &str) -> String {
    let out = scratch.join(format!("{name}.ll"));
    let mut command = Command::new(clang);
    command.args([flag, "-O0", "--target=aarch64-linux-gnu", "-S", "-emit-llvm", "-w"]);
    if source.extension().is_some_and(|e| e == "cpp") {
        command.arg("-std=c++20");
    }
    command.arg(source).arg("-o").arg(&out);
    output(&mut command);
    support::strip_template_params(&std::fs::read_to_string(out).unwrap())
}

fn hod(scratch: &Scratch, name: &str, text: &str, compression: Compression) -> PathBuf {
    let (module, bodies) = parse(text);
    let options = Options { debug_compression: compression, ..Options::default() };
    let bytes = hod_a64::compile_to_object(&module, &bodies, &options)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    scratch.write(&format!("{name}.o"), bytes)
}

/// A section of an ELF file: name, flags, alignment, bytes.
struct Section {
    name: String,
    flags: u64,
    align: u64,
    data: Vec<u8>,
}

fn sections(object: &Path) -> Vec<Section> {
    let file = std::fs::read(object).unwrap();
    let u16_at = |o: usize| u16::from_le_bytes(file[o..o + 2].try_into().unwrap()) as usize;
    let u32_at = |o: usize| u32::from_le_bytes(file[o..o + 4].try_into().unwrap()) as usize;
    let u64_at = |o: usize| u64::from_le_bytes(file[o..o + 8].try_into().unwrap());
    let shoff = u64_at(0x28) as usize;
    let (shnum, shstrndx) = (u16_at(0x3c), u16_at(0x3e));
    let header = |i: usize| shoff + 64 * i;
    let strtab = u64_at(header(shstrndx) + 0x18) as usize;
    (1..shnum)
        .map(|i| {
            let h = header(i);
            let at = strtab + u32_at(h);
            let end = at + file[at..].iter().position(|&b| b == 0).unwrap();
            let nobits = u32_at(h + 4) == 8;
            let (offset, size) = (u64_at(h + 0x18) as usize, u64_at(h + 0x20) as usize);
            Section {
                name: String::from_utf8_lossy(&file[at..end]).into_owned(),
                flags: u64_at(h + 8),
                align: u64_at(h + 0x30),
                data: if nobits { Vec::new() } else { file[offset..offset + size].to_vec() },
            }
        })
        .collect()
}

const SHF_COMPRESSED: u64 = 0x800;

const INFLATE: &str = r"
import sys, zlib
for path in sys.argv[1:]:
    d = zlib.decompressobj()
    data = open(path, 'rb').read()
    out = d.decompress(data) + d.flush()
    assert d.eof and not d.unused_data, path
    open(path + '.out', 'wb').write(out)
";

/// Checks the compressed object against the plain one section by
/// section; returns (sections compressed, bytes before, bytes after).
fn same_but_compressed(scratch: &Scratch, plain: &Path, zlib: &Path) -> (usize, usize, usize) {
    let (p, z) = (sections(plain), sections(zlib));
    let names = |s: &[Section]| s.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&p), names(&z), "{}", zlib.display());
    let mut streams = Vec::new();
    let mut tally = (0, 0, 0);
    for (a, b) in p.iter().zip(&z) {
        if b.flags & SHF_COMPRESSED == 0 {
            assert!(a.data == b.data && a.flags == b.flags && a.align == b.align, "{}", a.name);
            continue;
        }
        assert!(a.name.starts_with(".debug_"), "{} compressed", a.name);
        assert_eq!(b.flags, a.flags | SHF_COMPRESSED, "{}", a.name);
        assert_eq!(b.align, 8, "{}", a.name);
        let word = |at: usize| u64::from_le_bytes(b.data[at..at + 8].try_into().unwrap());
        assert_eq!(word(0), 1, "{}: ch_type ELFCOMPRESS_ZLIB, ch_reserved 0", a.name);
        assert_eq!(word(8), a.data.len() as u64, "{}: ch_size", a.name);
        assert_eq!(word(16), a.align, "{}: ch_addralign", a.name);
        assert!(b.data.len() < a.data.len(), "{}: did not shrink", a.name);
        let stream = scratch.write(
            &format!("{}{}.z", zlib.file_stem().unwrap().to_string_lossy(), a.name),
            &b.data[24..],
        );
        streams.push((stream, &a.data));
        tally = (tally.0 + 1, tally.1 + a.data.len(), tally.2 + b.data.len());
    }
    if let Some(python) = tool("python3") {
        let script = scratch.write("inflate.py", INFLATE);
        let mut command = Command::new(python);
        command.arg("-I").arg(script).args(streams.iter().map(|s| &s.0));
        output(&mut command);
        for (stream, want) in &streams {
            let got = std::fs::read(format!("{}.out", stream.display())).unwrap();
            assert!(&got == *want, "{}: python's zlib decodes other bytes", stream.display());
        }
    }
    if let Some(objcopy) = tool("llvm-objcopy-22") {
        let out = scratch.join(format!("{}-dz.o", zlib.file_stem().unwrap().to_string_lossy()));
        output(Command::new(objcopy).arg("--decompress-debug-sections").arg(zlib).arg(&out));
        // llvm-objcopy rewrites the symbol and string tables; the debug
        // sections must come back as they were.
        let decoded = sections(&out);
        for a in p.iter().filter(|s| s.name.starts_with(".debug_")) {
            let d = decoded.iter().find(|d| d.name == a.name).expect("section kept");
            assert!(a.data == d.data, "{}: llvm-objcopy decodes other bytes", a.name);
        }
    }
    tally
}

/// `plain` with its debug sections compressed by `llvm-objcopy`.
fn llvm_compressed(scratch: &Scratch, plain: &Path) -> PathBuf {
    let objcopy = tool("llvm-objcopy-22").expect("llvm-objcopy-22");
    let out = scratch.join(format!("{}-llvmz.o", plain.file_stem().unwrap().to_string_lossy()));
    output(Command::new(objcopy).arg("--compress-debug-sections=zlib").arg(plain).arg(&out));
    out
}

/// What a tool prints for `file`, with its path masked; its error stream
/// must be empty.
fn printed(command: &mut Command, file: &Path) -> String {
    let out = run_with(command, &[], Duration::from_mins(3));
    assert!(out.success(), "{command:?}: {}", out.stderr_text());
    assert_eq!(out.stderr_text(), "", "{command:?}");
    let dir = file.parent().unwrap().display().to_string();
    out.stdout_text()
        .replace(&file.display().to_string(), "FILE")
        .replace(&dir, "DIR")
        .replace(&file.file_name().unwrap().to_string_lossy().to_string(), "FILE")
}

fn readers_agree(plain: &Path, zlib: &Path) {
    for major in [19, 22] {
        let Some(dwarfdump) = tool(&format!("llvm-dwarfdump-{major}")) else { continue };
        support::verify(&dwarfdump, zlib);
        let dump = |f: &Path| printed(Command::new(&dwarfdump).arg("--all").arg(f), f);
        assert_eq!(dump(plain), dump(zlib), "llvm-dwarfdump-{major} --all {}", zlib.display());
    }
    if let Some(readelf) = tool("readelf") {
        let dump = |f: &Path| printed(Command::new(&readelf).arg("--debug-dump").arg(f), f);
        assert_eq!(dump(plain), dump(zlib), "readelf --debug-dump {}", zlib.display());
    }
}

/// A gdb batch session; addresses are equal for the two objects.
fn gdb(program: &Path, commands: &[String]) -> String {
    let gdb = tool("gdb").unwrap();
    let mut command = Command::new(gdb);
    command.args(["-nx", "-batch", "-ex", "set pagination off", "-ex", "set confirm off"]);
    command.args(["-ex", "set debuginfod enabled off", "-ex", "set style enabled off"]);
    for c in commands {
        command.arg("-ex").arg(c);
    }
    command.arg(program);
    let out = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(!out.timed_out(), "gdb on {}", program.display());
    mask_ids(&out.stdout_text(), program)
}

fn lldb(program: &Path, commands: &[String]) -> String {
    let lldb = tool("lldb").unwrap();
    let mut command = Command::new(lldb);
    command.args(["--no-lldbinit", "--batch", "-o", "settings set use-color false"]);
    for c in commands {
        command.arg("-o").arg(c);
    }
    command.arg(program);
    let out = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(!out.timed_out(), "lldb on {}", program.display());
    mask_ids(&out.stdout_text(), program)
}

/// Process ids, addresses (the programs run with ASLR) and the
/// program's path masked.
fn mask_ids(text: &str, program: &Path) -> String {
    let text = text
        .replace(&program.display().to_string(), "PROG")
        .replace(&program.parent().unwrap().display().to_string(), "DIR")
        .replace(&program.file_name().unwrap().to_string_lossy().to_string(), "PROG");
    let mut out = String::new();
    for line in text.lines() {
        let mut words = Vec::new();
        let mut pid = false;
        for w in line.split(' ') {
            if pid && w.trim_end_matches([',', ')', ']']).bytes().all(|b| b.is_ascii_digit()) {
                words.push("PID");
            } else {
                words.push(w);
            }
            pid = matches!(w, "process" | "Process" | "pid" | "(process");
        }
        let line = words.join(" ");
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i..].starts_with(b"0x") {
                out.push_str("0x?");
                i += 2;
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    i += 1;
                }
            } else {
                out.push(b[i] as char);
                i += 1;
            }
        }
        out.push('\n');
    }
    out
}

struct Case {
    name: &'static str,
    source: PathBuf,
    flag: &'static str,
    /// gdb and lldb commands on the object (`{}` is not used).
    gdb_object: &'static [&'static str],
    lldb_object: &'static [&'static str],
    /// Sessions on the program.
    gdb_program: &'static [&'static str],
    lldb_program: &'static [&'static str],
    /// What hod's sessions must show.
    wants: &'static [&'static str],
}

fn prog_case() -> Case {
    Case {
        name: "prog",
        source: data_dir().join("compressed/prog.cpp"),
        flag: "-g",
        gdb_object: &["info line accumulate", "ptype shapes::Rect", "info scope accumulate"],
        lldb_object: &["image lookup -n accumulate -v", "type lookup shapes::Rect"],
        gdb_program: &[
            "break prog.cpp:14",
            "run",
            "bt",
            "info locals",
            "print *this",
            "finish",
            "delete",
            "break shapes::weight",
            "continue",
            "info args",
            "print total",
            "continue",
        ],
        lldb_program: &[
            "breakpoint set --file prog.cpp --line 14",
            "run",
            "bt",
            "frame variable",
            "breakpoint delete --force",
            "breakpoint set --name shapes::weight",
            "continue",
            "frame variable",
            "continue",
        ],
        wants: &["a = 6", "width = 2", "c = shapes::Colour::Green", "base = 68", "exited normally"],
    }
}

fn lto_case() -> Case {
    Case {
        name: "lto",
        source: data_dir().join("reduced/lto.cpp"),
        flag: "-gline-tables-only",
        gdb_object: &["info line lto.cpp:14"],
        lldb_object: &["image lookup -n main -v"],
        gdb_program: &["break lto.cpp:14", "run", "bt", "delete", "continue"],
        lldb_program: &["breakpoint set --file lto.cpp --line 14", "run", "bt", "continue"],
        wants: &["fact(int) () at ", "exited normally"],
    }
}

fn run_case(case: &Case) {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the programs are built for AArch64; skipping on this host");
        return;
    }
    let scratch = Scratch::new(&format!("compressed-{}", case.name));
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    let debuggers = tool("gdb").is_some() && tool("lldb").is_some();
    let mut tally = (0, 0, 0);
    let mut sessions = 0;
    for major in [19, 22] {
        let Some(clang) = tool(&format!("clang++-{major}")) else { continue };
        let text = ir(&clang, &scratch, &format!("{}-{major}", case.name), &case.source, case.flag);
        let tag = format!("{}-{major}", case.name);
        let plain = hod(&scratch, &format!("{tag}-plain"), &text, Compression::None);
        let zlib = hod(&scratch, &format!("{tag}-zlib"), &text, Compression::Zlib);
        let counts = same_but_compressed(&scratch, &plain, &zlib);
        assert!(counts.0 > 0, "{tag}: nothing was compressed");
        tally = (tally.0 + counts.0, tally.1 + counts.1, tally.2 + counts.2);
        readers_agree(&plain, &zlib);
        let own = |s: &[&str]| s.iter().map(|&c| c.to_owned()).collect::<Vec<_>>();
        if debuggers {
            assert_eq!(gdb(&plain, &own(case.gdb_object)), gdb(&zlib, &own(case.gdb_object)));
            // lldb 19 reads no DWARF from a relocatable object whose debug
            // sections are compressed, LLVM's own included (`clang -gz=zlib`
            // objects: "no type was found"): on the object, hod's must read
            // as LLVM's compression of the same object does.
            let llvm = llvm_compressed(&scratch, &plain);
            assert_eq!(lldb(&llvm, &own(case.lldb_object)), lldb(&zlib, &own(case.lldb_object)));
            sessions += 4;
        }
        for linker in &toolchain.linkers {
            let dir = scratch.join(format!("{tag}-{}", linker.name));
            std::fs::create_dir_all(&dir).unwrap();
            let mut programs = Vec::new();
            for (object, kind) in [(&plain, "plain"), (&zlib, "zlib")] {
                let program = dir.join(kind);
                let ran = toolchain.link_and_run(linker, &[], &[object], &program);
                assert_eq!(exit_code(&ran), Some(0), "{tag}, {kind}, {}", linker.name);
                programs.push(program);
            }
            let (p, z) = (&programs[0], &programs[1]);
            if let Some(dwarfdump) = tool("llvm-dwarfdump-22") {
                let dump = |f: &Path| {
                    let out = printed(
                        Command::new(&dwarfdump).args(["--debug-info", "--debug-line"]).arg(f),
                        f,
                    );
                    out.replace("plain", "PROG").replace("zlib", "PROG")
                };
                assert_eq!(dump(p), dump(z), "{tag}, {}: the programs' DWARF", linker.name);
            }
            if debuggers {
                let g = own(case.gdb_program);
                let (gp, gz) = (gdb(p, &g), gdb(z, &g));
                assert_eq!(gp, gz, "{tag}, {}: gdb", linker.name);
                let l = own(case.lldb_program);
                let (lp, lz) = (lldb(p, &l), lldb(z, &l));
                assert_eq!(lp, lz, "{tag}, {}: lldb", linker.name);
                for w in case.wants {
                    assert!(
                        gz.contains(w) || lz.contains(w),
                        "{tag}, {}: {w:?} in\n{gz}\n{lz}",
                        linker.name
                    );
                }
                sessions += 4;
            }
        }
    }
    eprintln!(
        "{}: {} sections compressed, {} bytes into {}; {sessions} debugger sessions equal",
        case.name, tally.0, tally.1, tally.2
    );
}

#[test]
fn full_debug_program_compressed() {
    run_case(&prog_case());
}

#[test]
fn line_tables_program_compressed() {
    run_case(&lto_case());
}

/// A generated C program of many functions: sections past 64 KiB and
/// past a compressor segment (256 KiB), through every reader.
#[test]
fn large_sections_compressed() {
    if !cfg!(target_arch = "aarch64") {
        return;
    }
    let Some(clang) = tool("clang-22") else { return };
    let scratch = Scratch::new("compressed-large");
    let mut source =
        String::from("struct pair_of_values { int first_value; long second_value; };\n");
    for i in 0..6000 {
        writeln!(
            source,
            "int generated_function_with_a_long_name_{i}(int argument_{i}) {{ struct pair_of_values local_{i} = {{argument_{i}, {i}}}; return local_{i}.first_value + (int)local_{i}.second_value; }}"
        )
        .unwrap();
    }
    source.push_str("int main(void) { return generated_function_with_a_long_name_7(1) != 8; }\n");
    let path = scratch.write("large.c", source);
    let text = ir(&clang, &scratch, "large", &path, "-g");
    let plain = hod(&scratch, "large-plain", &text, Compression::None);
    let zlib = hod(&scratch, "large-zlib", &text, Compression::Zlib);
    let big = sections(&plain).iter().filter(|s| s.data.len() > 256 * 1024).count();
    assert!(big >= 2, "{big} sections past 256 KiB");
    let t = same_but_compressed(&scratch, &plain, &zlib);
    readers_agree(&plain, &zlib);
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    for linker in &toolchain.linkers {
        let program = scratch.join(format!("large-{}", linker.name));
        let ran = toolchain.link_and_run(linker, &[], &[&zlib], &program);
        assert_eq!(exit_code(&ran), Some(0), "{}", linker.name);
    }
    eprintln!("large: {} sections compressed, {} bytes into {}", t.0, t.1, t.2);
}
