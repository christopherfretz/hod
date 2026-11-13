//! Thread-local storage and symbol offsets (WP-073, `spec/codegen.md`
//! §5.5.1 "Thread-local storage and symbol offsets (WP-073)"): every
//! TLS model as AArch64 Linux `llc` writes it, linked into executables
//! (static, non-PIE, PIE) and shared objects under bfd, lld and mold and
//! run with several threads; the linkers' relaxations read back from the
//! linked code; the relocations compared with `llc`'s; and constant
//! offsets from symbols, folded only as `llc` folds them.

#![allow(clippy::too_many_lines, clippy::too_many_arguments)]

mod common;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Linker, Toolchain, exit_code};
use common::{emit, ll_header, llc, llc_major, parse, write_object};
use hod_cg::{Options, RelocModel};
use hod_testutil::{Scratch, run_with, tool};

/// A file of `tests/data/tls`.
fn data(name: &str) -> String {
    let path = format!("{}/tests/data/tls/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// How a module is built: relocation model and module flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    /// `-relocation-model=static`, no PIC level: an executable.
    Static,
    /// PIC with `PIE Level` 2: a position-independent executable.
    Pie,
    /// PIC with no `PIE Level`: a shared object.
    Shared,
}

impl Mode {
    fn flags(self) -> &'static str {
        match self {
            Mode::Static => "",
            Mode::Pie => {
                "!llvm.module.flags = !{!900, !901}\n!900 = !{i32 8, !\"PIC Level\", i32 2}\n\
                 !901 = !{i32 7, !\"PIE Level\", i32 2}\n"
            }
            Mode::Shared => {
                "!llvm.module.flags = !{!900}\n!900 = !{i32 8, !\"PIC Level\", i32 2}\n"
            }
        }
    }

    fn options(self) -> Options {
        match self {
            Mode::Static => Options::default(),
            Mode::Pie | Mode::Shared => {
                Options { reloc_model: RelocModel::Pic, ..Options::default() }
            }
        }
    }

    fn llc_flags(self) -> &'static [&'static str] {
        match self {
            Mode::Static => &[],
            Mode::Pie | Mode::Shared => &["-relocation-model=pic"],
        }
    }
}

/// The object of `text` built by hod or by `llc` in `mode`.
fn object(dir: &Scratch, name: &str, text: &str, mode: Mode, with_llc: bool) -> Option<PathBuf> {
    if with_llc {
        let full = format!("{}{text}{}", ll_header(llc_major()), mode.flags());
        return llc(dir, name, &full, mode.llc_flags());
    }
    let (module, bodies) = parse(&format!("{}{text}{}", ll_header(22), mode.flags()));
    let emitted = emit(&module, &bodies, &mode.options());
    Some(write_object(dir, &format!("{name}.o"), &emitted))
}

/// Runs a tool, requiring success; its standard output.
fn output(program: &Path, args: &[&std::ffi::OsStr]) -> String {
    let mut command = Command::new(program);
    command.args(args);
    let ran = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(ran.success(), "{}: {}", program.display(), ran.stderr_text());
    ran.stdout_text()
}

/// The TLS relocations of an object: `type symbol` → count. A local
/// alias (`.L<name>$local`) counts as its symbol.
fn tls_relocations(object: &Path) -> Option<BTreeMap<String, usize>> {
    let readelf = tool("llvm-readelf")?;
    let text = output(&readelf, &["-r".as_ref(), object.as_os_str()]);
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 5 && fields[2].contains("_TLS") {
            let sym = fields[4].trim_start_matches(".L").trim_end_matches("$local");
            *out.entry(format!("{} {sym}", fields[2])).or_insert(0) += 1;
        }
    }
    Some(out)
}

/// The instructions of the functions named `prefix*` in a linked file,
/// as `llvm-objdump -d` prints them without addresses and bytes.
fn disassembly(file: &Path, prefix: &str) -> Option<Vec<String>> {
    let objdump = tool("llvm-objdump")?;
    let text = output(
        &objdump,
        &[
            "-d".as_ref(),
            "--no-show-raw-insn".as_ref(),
            "--no-leading-addr".as_ref(),
            file.as_os_str(),
        ],
    );
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.ends_with(">:") {
            inside = line.starts_with(&format!("<{prefix}"));
            continue;
        }
        if inside && line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
            let line = line.split("//").next().unwrap_or(line);
            out.push(line.split_whitespace().collect::<Vec<_>>().join(" "));
        }
    }
    Some(out)
}

/// What the linked library code says about the TLS sequences in it.
#[derive(Debug, Default)]
struct Sequences {
    /// Descriptor calls left as they are (`blr x1`).
    descriptor_calls: usize,
    /// Descriptor or initial-exec sequences relaxed to local exec
    /// (`movz xN, #..., lsl #16` + `movk`).
    to_local_exec: usize,
    /// Descriptor sequences relaxed to initial exec (`adrp x0` + `ldr
    /// x0, [x0, ...]`, and two `nop`s).
    to_initial_exec: usize,
}

fn sequences(code: &[String]) -> Sequences {
    let mut s = Sequences::default();
    for (i, line) in code.iter().enumerate() {
        let next = code.get(i + 1).map_or("", String::as_str);
        if line == "blr x1" {
            s.descriptor_calls += 1;
        }
        if line.starts_with("movz x") && line.ends_with("lsl #16") && next.starts_with("movk x") {
            s.to_local_exec += 1;
        }
        // bfd and lld keep the GOT load in place of `adrp` + `ldr`,
        // mold moves it after two `nop`s.
        if line.starts_with("adrp x0,") && next.starts_with("ldr x0, [x0") {
            s.to_initial_exec += 1;
        }
    }
    s
}

/// One way of building and linking the program of `data/tls`.
struct Build {
    tag: &'static str,
    /// The library module's mode, and whether it becomes a shared object.
    lib: Mode,
    lib_shared: bool,
    /// The executable module's mode (`None`: not linked in).
    main: Option<Mode>,
    /// `dso.c` as a shared object rather than an object of the program.
    dso_shared: bool,
    /// The link flags of the executable.
    exe_flags: &'static [&'static str],
    /// The C flags of the driver.
    c_flags: &'static [&'static str],
}

const BUILDS: [Build; 5] = [
    // Executable, non-PIE: initial exec and local exec in the objects,
    // initial exec relaxed to local exec by the linker.
    Build {
        tag: "exe",
        lib: Mode::Static,
        lib_shared: false,
        main: Some(Mode::Static),
        dso_shared: true,
        exe_flags: &["-no-pie"],
        c_flags: &["-fno-pie", "-DWITH_MAIN"],
    },
    Build {
        tag: "pie",
        lib: Mode::Pie,
        lib_shared: false,
        main: Some(Mode::Pie),
        dso_shared: true,
        exe_flags: &["-pie"],
        c_flags: &["-fPIE", "-DWITH_MAIN"],
    },
    // The library a shared object: descriptors, kept by the linker; a
    // variable of the executable and one of another shared object used
    // from it, and one of it used from the executable.
    Build {
        tag: "shared",
        lib: Mode::Shared,
        lib_shared: true,
        main: Some(Mode::Static),
        dso_shared: true,
        exe_flags: &["-no-pie"],
        c_flags: &["-fno-pie", "-DWITH_MAIN"],
    },
    // Descriptors linked into an executable: relaxed to local exec for
    // its own variables and to initial exec for another object's.
    Build {
        tag: "relax",
        lib: Mode::Shared,
        lib_shared: false,
        main: None,
        dso_shared: true,
        exe_flags: &["-pie"],
        c_flags: &["-fPIE"],
    },
    // Statically linked: every sequence relaxed to local exec.
    Build {
        tag: "static",
        lib: Mode::Shared,
        lib_shared: false,
        main: Some(Mode::Static),
        dso_shared: false,
        exe_flags: &["-static"],
        c_flags: &["-fno-pie", "-DWITH_MAIN"],
    },
];

/// Links and runs one build with one linker; returns the linked file
/// that holds the library's code.
fn link_run(
    toolchain: &Toolchain,
    dir: &Scratch,
    linker: &Linker,
    build: &Build,
    who: &str,
    objects: (&Path, Option<&Path>),
    driver: &Path,
    dso: (&Path, &Path),
) -> PathBuf {
    let tag = format!("{}-{who}-{}", build.tag, linker.name);
    let rpath = format!("-Wl,-rpath,{}", dir.path().display());
    let (lib_obj, main_obj) = objects;
    let mut inputs: Vec<PathBuf> = vec![driver.to_path_buf()];
    inputs.extend(main_obj.map(Path::to_path_buf));
    let dso_so = dir.join(format!("libdso-{}.so", linker.name));
    if build.dso_shared && !dso_so.exists() {
        toolchain.link_ok(linker, &["-shared"], &[dso.1], &dso_so);
    }
    let lib_file = if build.lib_shared {
        let so = dir.join(format!("liblib-{tag}.so"));
        let mut lib_inputs = vec![lib_obj];
        if build.dso_shared {
            lib_inputs.push(&dso_so);
        }
        toolchain.link_ok(linker, &["-shared"], &lib_inputs, &so);
        inputs.push(so.clone());
        so
    } else {
        inputs.push(lib_obj.to_path_buf());
        dir.join(format!("p-{tag}"))
    };
    inputs.push(if build.dso_shared { dso_so } else { dso.0.to_path_buf() });
    let exe = dir.join(format!("p-{tag}"));
    let mut flags: Vec<&str> = build.exe_flags.to_vec();
    flags.extend(["-pthread", rpath.as_str()]);
    let refs: Vec<&Path> = inputs.iter().map(PathBuf::as_path).collect();
    let ran = toolchain.link_and_run(linker, &flags, &refs, &exe);
    let out = ran.stdout_text();
    assert!(
        exit_code(&ran) == Some(0) && out.contains("checked"),
        "{tag}: exit {:?}\n{out}{}",
        exit_code(&ran),
        ran.stderr_text()
    );
    lib_file
}

/// `spec/codegen.md` §5.5.1 (WP-073): every model, five ways of linking,
/// three linkers, hod's objects and `llc`'s; the relaxations each linker
/// applies to hod's sequences; the relocations against `llc`'s.
#[test]
fn every_model_links_runs_and_relaxes() {
    let dir = Scratch::new("a64-tls");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let lib = data("lib.ll");
    let main = data("main.ll");
    let dso_src = "__thread int dso_tls = 200;\n";
    let dso_obj = toolchain.compile_c(&dir, "dso", dso_src, &["-fPIC"]);
    let dso_static = toolchain.compile_c(&dir, "dso-static", dso_src, &["-fno-pie"]);
    let driver_src = data("driver.c");
    let mut report = String::new();
    for build in &BUILDS {
        let driver =
            toolchain.compile_c(&dir, &format!("driver-{}", build.tag), &driver_src, build.c_flags);
        let dso = (if build.dso_shared { &dso_obj } else { &dso_static }, &dso_obj);
        for who in ["hod", "llc"] {
            let with_llc = who == "llc";
            let name = format!("lib-{}-{who}", build.tag);
            let Some(lib_obj) = object(&dir, &name, &lib, build.lib, with_llc) else { return };
            let main_obj = match build.main {
                Some(mode) => {
                    let name = format!("main-{}-{who}", build.tag);
                    let Some(obj) = object(&dir, &name, &main, mode, with_llc) else { return };
                    Some(obj)
                }
                None => None,
            };
            for linker in &toolchain.linkers {
                let file = link_run(
                    &toolchain,
                    &dir,
                    linker,
                    build,
                    who,
                    (&lib_obj, main_obj.as_deref()),
                    &driver,
                    (dso.0.as_path(), dso.1.as_path()),
                );
                let Some(code) = disassembly(&file, "lib_") else { continue };
                let seq = sequences(&code);
                let _ = writeln!(report, "{} {who} {}: {seq:?}", build.tag, linker.name);
                if who == "llc" {
                    continue;
                }
                let tag = format!("{} {}", build.tag, linker.name);
                match build.tag {
                    // The shared object keeps every descriptor call: one
                    // per access of lib_gd, lib_local, lib_ld, exe_tls,
                    // dso_tls and the arrays.
                    "shared" => assert!(seq.descriptor_calls >= 20, "{tag}: {seq:?}"),
                    "relax" => {
                        assert_eq!(seq.descriptor_calls, 0, "{tag}: {seq:?}");
                        assert!(seq.to_local_exec >= 10, "{tag}: {seq:?}");
                        assert!(seq.to_initial_exec >= 5, "{tag}: {seq:?}");
                    }
                    // bfd and lld relax every one to local exec; mold
                    // takes the symbols defined in another object to
                    // initial exec (a GOT entry the static link fills).
                    "static" => {
                        assert_eq!(seq.descriptor_calls, 0, "{tag}: {seq:?}");
                        assert!(seq.to_local_exec >= 10, "{tag}: {seq:?}");
                        assert!(seq.to_local_exec + seq.to_initial_exec >= 20, "{tag}: {seq:?}");
                    }
                    // Initial exec relaxed to local exec: bfd and lld
                    // do it, mold keeps the GOT entry.
                    _ if linker.name != "mold" => {
                        assert!(seq.to_local_exec >= 10, "{tag}: {seq:?}");
                    }
                    _ => {}
                }
            }
        }
        // The relocations: the models `llc` chose, symbol for symbol.
        let hod_obj = dir.join(format!("lib-{}-hod.o", build.tag));
        let llc_obj = dir.join(format!("lib-{}-llc.o", build.tag));
        if let (Some(ours), Some(theirs)) = (tls_relocations(&hod_obj), tls_relocations(&llc_obj)) {
            assert_eq!(
                ours.keys().collect::<Vec<_>>(),
                theirs.keys().collect::<Vec<_>>(),
                "{}",
                build.tag
            );
        }
    }
    eprintln!("{report}");
}

// ---- constant offsets from symbols ----

/// The symbols of the offset table: `(IR name, IR declaration, C
/// declaration, size)`; size 0 for an unsized one.
const OFFSET_SYMBOLS: [(&str, &str, &str, i64); 6] = [
    ("small", "dso_local global [16 x i8] zeroinitializer, align 16", "extern char small[16];", 16),
    (
        "big",
        "dso_local global [8192 x i8] zeroinitializer, align 16",
        "extern char big[8192];",
        8192,
    ),
    ("pre", "global [16 x i8] zeroinitializer, align 16", "extern char pre[16];", 16),
    ("ext", "external global [16 x i8], align 16", "char ext[16];", 16),
    ("w", "extern_weak global i8", "extern char w __attribute__((weak));", 1),
    ("fn", "", "void fn(void);", 0),
];

const OFFSETS: [i64; 19] = [
    0,
    1,
    15,
    16,
    17,
    8192,
    8193,
    -1,
    -16,
    4095,
    4096,
    (1 << 20) - 1,
    1 << 20,
    1 << 32,
    -(1 << 32),
    300_000_000_000_000_000,
    -300_000_000_000_000_000,
    i64::MAX,
    i64::MIN,
];

/// The IR module and the C driver of the offset table: one function per
/// symbol and offset returning `sym + off`, and one loading the byte
/// there for each offset inside a defined object.
fn offset_table() -> (String, String) {
    let mut ir = String::new();
    let mut c = String::from("#include <stdint.h>\n#include <stdio.h>\n");
    let mut cases = String::new();
    let mut loads = String::new();
    for (name, decl, c_decl, _) in OFFSET_SYMBOLS {
        if name == "fn" {
            let _ = writeln!(ir, "define dso_local void @fn() {{\n  ret void\n}}");
        } else {
            let _ = writeln!(ir, "@{name} = {decl}");
        }
        let _ = writeln!(c, "{c_decl}");
    }
    for (s, (name, _, _, size)) in OFFSET_SYMBOLS.iter().enumerate() {
        let base =
            if *name == "fn" { "(uintptr_t)fn".to_owned() } else { format!("(uintptr_t)&{name}") };
        for (k, off) in OFFSETS.iter().enumerate() {
            let _ = writeln!(
                ir,
                "define ptr @o_{s}_{k}() {{\n  ret ptr getelementptr (i8, ptr @{name}, i64 {off})\n}}"
            );
            let _ = writeln!(c, "void *o_{s}_{k}(void);");
            let _ =
                writeln!(cases, "    {{o_{s}_{k}, {base}, (uint64_t){off}LL, \"{name}{off:+}\"}},");
            if (0..*size).contains(off) && matches!(*name, "small" | "big" | "pre" | "ext") {
                let _ = writeln!(
                    ir,
                    "define i8 @l_{s}_{k}() {{\n  %v = load i8, ptr getelementptr (i8, ptr @{name}, i64 {off})\n  ret i8 %v\n}}"
                );
                let _ = writeln!(c, "char l_{s}_{k}(void);");
                let _ =
                    writeln!(loads, "    {{l_{s}_{k}, (char *){base}, {off}, \"{name}{off:+}\"}},");
            }
        }
    }
    let _ = write!(
        c,
        "struct {{ void *(*f)(void); uintptr_t base; uint64_t off; const char *what; }} cases[] = {{\n{cases}}};\n\
         struct {{ char (*f)(void); char *base; long off; const char *what; }} loads[] = {{\n{loads}}};\n\
         int main(void) {{\n\
         \x20   int bad = 0;\n\
         \x20   for (int i = 0; i < 16; i++) {{ small[i] = 1 + i; pre[i] = 40 + i; ext[i] = 80 + i; }}\n\
         \x20   for (int i = 0; i < 8192; i++) big[i] = (char)(i * 7 + 3);\n\
         \x20   for (unsigned i = 0; i < sizeof cases / sizeof cases[0]; i++) {{\n\
         \x20       uintptr_t got = (uintptr_t)cases[i].f(), want = cases[i].base + cases[i].off;\n\
         \x20       if (got != want) {{ printf(\"FAIL %s: %#lx, not %#lx\\n\", cases[i].what, got, want); bad = 1; }}\n\
         \x20   }}\n\
         \x20   for (unsigned i = 0; i < sizeof loads / sizeof loads[0]; i++) {{\n\
         \x20       if (loads[i].f() != loads[i].base[loads[i].off]) {{ printf(\"FAIL load %s\\n\", loads[i].what); bad = 1; }}\n\
         \x20   }}\n\
         \x20   if (!bad) printf(\"checked\\n\");\n\
         \x20   return bad;\n\
         }}\n"
    );
    (ir, c)
}

/// The `adrp` relocations of an object: `type symbol addend`, sorted.
fn page_relocations(object: &Path) -> Option<Vec<String>> {
    let readelf = tool("llvm-readelf")?;
    let text = output(&readelf, &["-r".as_ref(), object.as_os_str()]);
    let mut out: Vec<String> = text
        .lines()
        .filter(|line| line.contains("ADR_PREL_PG_HI21") || line.contains("ADR_GOT_PAGE"))
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            fields[2..].join(" ")
        })
        .collect();
    out.sort();
    Some(out)
}

/// `spec/codegen.md` §5.5.1 (WP-073): a constant offset from a symbol is
/// folded into the `adrp`/`add` relocations exactly when `llc` folds it
/// (the relocations are `llc`'s, addend for addend), and any other is
/// added in code: every address in the table is right under three
/// linkers, static and PIE, including offsets no relocation could hold
/// and offsets from an undefined weak symbol and a function.
#[test]
fn symbol_offsets_fold_as_llc_folds() {
    let dir = Scratch::new("a64-offsets");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let (ir, c) = offset_table();
    for (mode, c_flags, link) in
        [(Mode::Static, "-fno-pie", "-no-pie"), (Mode::Pie, "-fPIE", "-pie")]
    {
        let tag = format!("{mode:?}");
        let driver = toolchain.compile_c(&dir, &format!("offsets-{tag}"), &c, &[c_flags, "-w"]);
        let ours = object(&dir, &format!("offsets-hod-{tag}"), &ir, mode, false).unwrap();
        let Some(theirs) = object(&dir, &format!("offsets-llc-{tag}"), &ir, mode, true) else {
            return;
        };
        if let (Some(a), Some(b)) = (page_relocations(&ours), page_relocations(&theirs)) {
            assert_eq!(a, b, "{tag}: hod's adrp relocations are llc's");
        }
        for linker in &toolchain.linkers {
            for (who, obj) in [("hod", &ours), ("llc", &theirs)] {
                let exe = dir.join(format!("offsets-{tag}-{who}-{}", linker.name));
                let ran = toolchain.link_and_run(linker, &[link], &[&driver, obj], &exe);
                assert!(
                    exit_code(&ran) == Some(0) && ran.stdout_text().contains("checked"),
                    "{tag} {who} {}: {}{}",
                    linker.name,
                    ran.stdout_text(),
                    ran.stderr_text()
                );
            }
        }
    }
}

// ---- the sequences, instruction for instruction ----

/// `llvm-objdump -dr` of an object: instruction and relocation lines,
/// addresses, bytes and comments dropped.
fn with_relocations(object: &Path) -> Option<Vec<String>> {
    let objdump = tool("llvm-objdump")?;
    let text = output(
        &objdump,
        &[
            "-dr".as_ref(),
            "--no-show-raw-insn".as_ref(),
            "--no-leading-addr".as_ref(),
            object.as_os_str(),
        ],
    );
    Some(
        text.lines()
            .filter(|line| line.starts_with(char::is_whitespace) && !line.trim().is_empty())
            .map(|line| {
                let line = line.split("//").next().unwrap_or(line);
                let line = line.trim();
                let line = match line.split_once(':') {
                    Some((at, rest)) if at.chars().all(|c| c.is_ascii_hexdigit()) => rest,
                    _ => line,
                };
                line.split_whitespace().collect::<Vec<_>>().join(" ")
            })
            .collect(),
    )
}

/// `spec/codegen.md` §5.5.1 (WP-073): the sequence of each model is
/// `llc`'s, relocation for relocation, around what hod saves.
#[test]
fn each_model_is_the_psabi_sequence() {
    let dir = Scratch::new("a64-tls-seq");
    let ir = "@gd = thread_local global i32 1\n\
              @ie = thread_local(initialexec) global i32 2\n\
              @le = dso_local thread_local(localexec) global i32 3\n\
              declare ptr @llvm.threadlocal.address.p0(ptr)\n\
              define ptr @f_gd() {\n  %p = call ptr @llvm.threadlocal.address.p0(ptr @gd)\n  ret ptr %p\n}\n\
              define ptr @f_ie() {\n  %p = call ptr @llvm.threadlocal.address.p0(ptr @ie)\n  ret ptr %p\n}\n\
              define ptr @f_le() {\n  %p = call ptr @llvm.threadlocal.address.p0(ptr @le)\n  ret ptr %p\n}\n";
    let obj = object(&dir, "seq", ir, Mode::Shared, false).unwrap();
    let Some(lines) = with_relocations(&obj) else { return };
    let text = lines.join("\n");
    let expect = |want: &[&str]| {
        assert!(text.contains(&want.join("\n")), "missing\n{}\nin\n{text}", want.join("\n"));
    };
    expect(&[
        "stp x0, x1, [sp, #-0x20]!",
        "mrs x1, NZCV",
        "str x1, [sp, #0x10]",
        "adrp x0, 0x0 <f_gd>",
        "R_AARCH64_TLSDESC_ADR_PAGE21 gd",
        "ldr x1, [x0]",
        "R_AARCH64_TLSDESC_LD64_LO12 gd",
        "add x0, x0, #0x0",
        "R_AARCH64_TLSDESC_ADD_LO12 gd",
        "blr x1",
        "R_AARCH64_TLSDESC_CALL gd",
        "mrs x1, TPIDR_EL0",
    ]);
    expect(&[
        "R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21 ie",
        "ldr x9, [x9]",
        "R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC ie",
        "str x0, [sp, #-0x10]!",
        "mrs x0, TPIDR_EL0",
        "add x9, x0, x9",
        "ldr x0, [sp], #0x10",
    ]);
    let le =
        lines.iter().position(|l| l.contains("TLSLE_ADD_TPREL_HI12 .Lle$local")).expect("hi12");
    assert!(lines[le - 1].ends_with("#0x0, lsl #12"), "{}", lines[le - 1]);
    assert!(lines[le - 2].starts_with("mrs x") && lines[le - 2].ends_with("TPIDR_EL0"));
    assert!(lines[le + 2].contains("TLSLE_ADD_TPREL_LO12_NC .Lle$local"), "{}", lines[le + 2]);
}
