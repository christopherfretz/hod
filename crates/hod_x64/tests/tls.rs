//! Thread-local storage on x86-64 (WP-134, `spec/codegen.md` §5.6.11):
//! every model as `llc -relocation-model=pic` writes it, linked into
//! executables (non-PIE, PIE, static, static PIE) and shared objects under
//! GNU ld, lld and mold and run under several threads (under
//! `qemu-x86_64` on another host); the linkers' relaxations read back
//! from the linked code; the relocations compared with `llc`'s; objects
//! of hod and `llc` mixed; the sequences byte for byte.

#![allow(clippy::too_many_lines, clippy::too_many_arguments)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{CgError, Options, RelocModel};
use hod_ir::{Body, Module};
use hod_testutil::{Scratch, find_tool, run_with, tool};
use linking::{Linker, Toolchain, exit_code};

/// The module header: the x86-64 data layout and triple.
fn header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// A module and its bodies from `.ll` text (the header added).
fn parse(text: &str) -> (Module, Vec<Body>) {
    let text = format!("{}{text}", header());
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, parsed| {
        let body =
            parsed.body.unwrap_or_else(|| panic!("hod_ll: unsupported {:?}", parsed.unsupported));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    (module, bodies)
}

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

    fn llc_model(self) -> &'static str {
        match self {
            Mode::Static => "-relocation-model=static",
            Mode::Pie | Mode::Shared => "-relocation-model=pic",
        }
    }
}

/// Who builds an object: hod, or `llc` of one LLVM major.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Who {
    Hod,
    Llc(u32),
}

/// The object of `text` (header and `mode`'s flags added); `None` when
/// that `llc` is not installed.
fn object(dir: &Scratch, name: &str, text: &str, mode: Mode, who: Who) -> Option<PathBuf> {
    let full = format!("{text}{}", mode.flags());
    match who {
        Who::Hod => {
            let (module, bodies) = parse(&full);
            let bytes = hod_x64::compile_to_object(&module, &bodies, &mode.options())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            Some(dir.write(&format!("{name}.o"), bytes))
        }
        Who::Llc(major) => {
            let llc = find_tool(&format!("llc-{major}"))?;
            let input = dir.write(&format!("{name}.ll"), format!("{}{full}", header()));
            let output = dir.join(format!("{name}.o"));
            let mut command = Command::new(llc);
            command
                .args(["-O0", "-fast-isel=0", "-filetype=obj", mode.llc_model()])
                .arg(&input)
                .arg("-o")
                .arg(&output);
            let ran = run_with(&mut command, &[], Duration::from_mins(2));
            assert!(ran.success(), "llc-{major} {name}: {}", ran.stderr_text());
            Some(output)
        }
    }
}

/// What compiling `text` as a shared object's module reports.
fn compile(text: &str, mode: Mode) -> Result<Vec<u8>, CgError> {
    let (module, bodies) = parse(&format!("{text}{}", mode.flags()));
    match hod_x64::compile_to_object(&module, &bodies, &mode.options()) {
        Ok(bytes) => Ok(bytes),
        Err(hod_x64::ModuleError::Function { error, .. }) => Err(error),
        Err(other) => panic!("{other}"),
    }
}

/// Runs a tool, requiring success; its standard output.
fn output(program: &Path, args: &[&std::ffi::OsStr]) -> String {
    let mut command = Command::new(program);
    command.args(args);
    let ran = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(ran.success(), "{}: {}", program.display(), ran.stderr_text());
    ran.stdout_text()
}

/// The TLS relocations of an object: `type symbol` → count (the call of
/// `__tls_get_addr` included). A local alias (`.L<name>$local`) counts
/// as its symbol.
fn tls_relocations(object: &Path) -> Option<BTreeMap<String, usize>> {
    let readelf = tool("llvm-readelf")?;
    let text = output(&readelf, &["-r".as_ref(), object.as_os_str()]);
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }
        let kind = fields[2];
        let tls = kind.contains("TLS") || kind.contains("TPOFF");
        if tls || (kind == "R_X86_64_PLT32" && fields[4] == "__tls_get_addr") {
            let sym = fields[4].trim_start_matches(".L").trim_end_matches("$local");
            *out.entry(format!("{kind} {sym}")).or_insert(0) += 1;
        }
    }
    Some(out)
}

/// The instructions of the functions named `prefix*` in a file, as
/// `llvm-objdump -d` prints them (AT&T syntax) without addresses and
/// bytes, with the relocation lines when `relocs`.
fn disassembly(file: &Path, prefix: &str, relocs: bool) -> Option<Vec<String>> {
    let objdump = tool("llvm-objdump")?;
    let flag = if relocs { "-dr" } else { "-d" };
    let text = output(
        &objdump,
        &[
            flag.as_ref(),
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
            let line = line.split('#').next().unwrap_or(line);
            let line = line.trim();
            let line = match line.split_once(':') {
                Some((at, rest)) if at.chars().all(|c| c.is_ascii_hexdigit()) => rest,
                _ => line,
            };
            out.push(line.split_whitespace().collect::<Vec<_>>().join(" "));
        }
    }
    Some(out)
}

/// What linked code says about the TLS sequences in it.
#[derive(Debug, Default, PartialEq, Eq)]
struct Sequences {
    /// Calls of `__tls_get_addr` left in place (general or local
    /// dynamic kept).
    dynamic_calls: usize,
    /// Loads of the thread pointer (`mov %fs:0x0, r`): every relaxed
    /// dynamic access and every static one.
    thread_pointer: usize,
    /// Dynamic accesses relaxed to initial exec: `add ...(%rip), r` or
    /// `mov ...(%rip), r` right after a thread-pointer load.
    from_got: usize,
    /// Offsets from the thread pointer the linker wrote as immediates:
    /// `lea -imm(r)` or `add $-imm` after a thread-pointer load (local
    /// exec, or a dynamic access relaxed to it), `mov $-imm, r` in place
    /// of the initial-exec GOT load.
    immediate: usize,
}

fn sequences(code: &[String]) -> Sequences {
    let mut s = Sequences::default();
    for (i, line) in code.iter().enumerate() {
        let next = code.get(i + 1).map_or("", String::as_str);
        if line.contains("call") && line.contains("__tls_get_addr") {
            s.dynamic_calls += 1;
        }
        if line.starts_with("movq %fs:0x0,") {
            s.thread_pointer += 1;
            if next.contains("(%rip),") && (next.starts_with("add") || next.starts_with("mov")) {
                s.from_got += 1;
            }
            if next.starts_with("leaq -0x") || next.starts_with("addq $-0x") {
                s.immediate += 1;
            }
        }
        // mold's local dynamic relaxed to local exec: `xor eax, eax; mov
        // rax, fs:[rax]`, then the offset subtracted.
        if line == "xorl %eax, %eax" && next.starts_with("movq %fs:(%rax), %rax") {
            s.thread_pointer += 1;
            s.immediate += 1;
        }
        // Initial exec relaxed in place: the GOT load became an immediate.
        if line.starts_with("movq $-0x") && next.starts_with("push") {
            s.immediate += 1;
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
    /// `dso_tls` in a shared object rather than an object of the program.
    dso_shared: bool,
    /// The link flags of the executable.
    exe_flags: &'static [&'static str],
    /// The C flags of the driver.
    c_flags: &'static [&'static str],
}

const BUILDS: [Build; 6] = [
    // Non-PIE executable: initial exec and local exec in the objects,
    // initial exec relaxed to local exec by bfd and lld.
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
    // The library a shared object: general and local dynamic through the
    // real `__tls_get_addr`; a variable of the executable and one of
    // another shared object used from it, and one of it used from the
    // executable.
    Build {
        tag: "shared",
        lib: Mode::Shared,
        lib_shared: true,
        main: Some(Mode::Static),
        dso_shared: true,
        exe_flags: &["-no-pie"],
        c_flags: &["-fno-pie", "-DWITH_MAIN"],
    },
    // The dynamic models linked into a PIE: relaxed to local exec for its
    // own variables and to initial exec for another object's.
    Build {
        tag: "relax",
        lib: Mode::Shared,
        lib_shared: false,
        main: None,
        dso_shared: true,
        exe_flags: &["-pie"],
        c_flags: &["-fPIE"],
    },
    // Statically linked: every sequence relaxed.
    Build {
        tag: "static",
        lib: Mode::Shared,
        lib_shared: false,
        main: Some(Mode::Static),
        dso_shared: false,
        exe_flags: &["-static"],
        c_flags: &["-fno-pie", "-DWITH_MAIN"],
    },
    Build {
        tag: "static-pie",
        lib: Mode::Shared,
        lib_shared: false,
        main: Some(Mode::Pie),
        dso_shared: false,
        exe_flags: &["-static-pie"],
        c_flags: &["-fPIE", "-DWITH_MAIN"],
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
    flags.push("-pthread");
    // GNU ld writes a `DT_RUNPATH` into a static PIE, whose start-up code
    // then crashes: a run path only where a shared object is loaded.
    if build.lib_shared || build.dso_shared {
        flags.push(rpath.as_str());
    }
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

/// The objects of one build: `(name, library object's builder, main's)`.
/// hod alone, `llc` 19 and 22 alone, and the two mixtures (hod's
/// accesses with `llc`'s definitions and the reverse).
const WHO: [(&str, Who, Who); 5] = [
    ("hod", Who::Hod, Who::Hod),
    ("llc19", Who::Llc(19), Who::Llc(19)),
    ("llc22", Who::Llc(22), Who::Llc(22)),
    ("hod-llc", Who::Hod, Who::Llc(22)),
    ("llc-hod", Who::Llc(22), Who::Hod),
];

/// `spec/codegen.md` §5.6.11 (WP-134): every model, six ways of
/// linking, three linkers, hod's objects, `llc`'s (19 and 22) and the
/// two mixed; the relaxations each linker applies to hod's sequences;
/// the relocations against `llc`'s.
#[test]
fn every_model_links_runs_and_relaxes() {
    let dir = Scratch::new("x64-tls");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let lib = data("lib.ll");
    let main = data("main.ll");
    let dso_src = "__thread int dso_tls = 200;\n";
    let dso_obj = toolchain.compile_c(&dir, "dso", dso_src, &["-fPIC"]);
    let dso_static = toolchain.compile_c(&dir, "dso-static", dso_src, &["-fPIE"]);
    let driver_src = data("driver.c");
    let mut report = String::new();
    for build in &BUILDS {
        let driver =
            toolchain.compile_c(&dir, &format!("driver-{}", build.tag), &driver_src, build.c_flags);
        let dso = (if build.dso_shared { &dso_obj } else { &dso_static }, &dso_obj);
        for (who, lib_who, main_who) in WHO {
            if who.contains('-') && build.main.is_none() {
                continue;
            }
            let name = format!("lib-{}-{who}", build.tag);
            let Some(lib_obj) = object(&dir, &name, &lib, build.lib, lib_who) else { continue };
            let main_obj = match build.main {
                Some(mode) => {
                    let name = format!("main-{}-{who}", build.tag);
                    let Some(obj) = object(&dir, &name, &main, mode, main_who) else { continue };
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
                let Some(code) = disassembly(&file, "lib_", false) else { continue };
                let seq = sequences(&code);
                let _ = writeln!(report, "{} {who} {}: {seq:?}", build.tag, linker.name);
                if who != "hod" {
                    continue;
                }
                let tag = format!("{} {}", build.tag, linker.name);
                let gnu = linker.name != "mold";
                match build.tag {
                    // The shared object keeps every dynamic call: one per
                    // access of a variable of the general or local dynamic
                    // model; the static models are the four left.
                    "shared" => {
                        assert!(seq.dynamic_calls >= 40, "{tag}: {seq:?}");
                        assert_eq!(seq.thread_pointer, 4, "{tag}: {seq:?}");
                    }
                    // Into a PIE: the variables of the executable relaxed to
                    // local exec, `dso_tls` to initial exec.
                    "relax" => {
                        assert_eq!(seq.dynamic_calls, 0, "{tag}: {seq:?}");
                        assert!(seq.immediate >= 15, "{tag}: {seq:?}");
                        assert!(seq.from_got >= 5, "{tag}: {seq:?}");
                    }
                    // Static: bfd and lld take every access to local exec;
                    // mold keeps the variables of another object in a GOT
                    // entry the static link fills.
                    "static" | "static-pie" => {
                        assert_eq!(seq.dynamic_calls, 0, "{tag}: {seq:?}");
                        assert!(seq.immediate + seq.from_got >= 30, "{tag}: {seq:?}");
                        assert!(!gnu || seq.from_got == 0 && seq.immediate >= 45, "{tag}: {seq:?}");
                    }
                    // Executables of the static models: initial exec of the
                    // executable's own variables relaxed to local exec.
                    _ => {
                        assert_eq!(seq.dynamic_calls, 0, "{tag}: {seq:?}");
                        assert!(seq.immediate >= if gnu { 40 } else { 30 }, "{tag}: {seq:?}");
                    }
                }
            }
        }
        // The relocations: the models `llc` chose, symbol for symbol.
        let ours = tls_relocations(&dir.join(format!("lib-{}-hod.o", build.tag)));
        let theirs = tls_relocations(&dir.join(format!("lib-{}-llc22.o", build.tag)));
        if let (Some(ours), Some(theirs)) = (ours, theirs) {
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

// ---- the sequences, byte for byte ----

/// `llvm-objdump -dr` of an object with the raw bytes: instruction and
/// relocation lines, addresses dropped.
fn with_bytes(object: &Path) -> Option<Vec<String>> {
    let objdump = tool("llvm-objdump")?;
    let text =
        output(&objdump, &["-dr".as_ref(), "--no-leading-addr".as_ref(), object.as_os_str()]);
    Some(
        text.lines()
            .filter(|line| line.starts_with(char::is_whitespace) && !line.trim().is_empty())
            .map(|line| {
                let line = line.split('#').next().unwrap_or(line);
                line.split_whitespace().collect::<Vec<_>>().join(" ")
            })
            .collect(),
    )
}

/// `spec/codegen.md` §5.6.11 (WP-134): the sequence of each model is
/// `llc`'s, byte for byte where the linkers read bytes (the padded
/// general-dynamic pair, the local-dynamic pair, the initial-exec load),
/// with `llc`'s relocations.
#[test]
fn each_model_is_the_psabi_sequence() {
    let dir = Scratch::new("x64-tls-seq");
    let ir = "@gd = thread_local global i32 1\n\
              @ld = internal thread_local global i32 5\n\
              @ie = thread_local(initialexec) global i32 2\n\
              @le = dso_local thread_local(localexec) global i32 3\n\
              declare ptr @llvm.threadlocal.address.p0(ptr)\n\
              declare ptr @llvm.thread.pointer()\n\
              define ptr @f_gd() {\n  %p = call ptr @llvm.threadlocal.address.p0(ptr @gd)\n  ret ptr %p\n}\n\
              define ptr @f_ld() {\n  %p = call ptr @llvm.threadlocal.address.p0(ptr @ld)\n  ret ptr %p\n}\n\
              define ptr @f_ie() {\n  %p = call ptr @llvm.threadlocal.address.p0(ptr @ie)\n  ret ptr %p\n}\n\
              define ptr @f_le() {\n  %p = call ptr @llvm.threadlocal.address.p0(ptr @le)\n  ret ptr %p\n}\n\
              define ptr @f_tp() {\n  %p = call ptr @llvm.thread.pointer()\n  ret ptr %p\n}\n\
              define ptr @f_op() {\n  ret ptr @gd\n}\n";
    let obj = object(&dir, "seq", ir, Mode::Shared, Who::Hod).unwrap();
    let Some(lines) = with_bytes(&obj) else { return };
    let text = lines.join("\n");
    let expect = |want: &[&str]| {
        assert!(text.contains(&want.join("\n")), "missing\n{}\nin\n{text}", want.join("\n"));
    };
    // General dynamic: exactly the 16 bytes the linkers rewrite.
    expect(&[
        "66 48 8d 3d 00 00 00 00 leaq (%rip), %rdi",
        "R_X86_64_TLSGD gd-0x4",
        "66 66 48 e8 00 00 00 00 callq 0x",
    ]);
    assert!(text.contains("R_X86_64_PLT32 __tls_get_addr-0x4"), "{text}");
    // Local dynamic: the 12 bytes, then the offset in the block.
    expect(&["48 8d 3d 00 00 00 00 leaq (%rip), %rdi", "R_X86_64_TLSLD ld-0x4"]);
    assert!(text.contains("R_X86_64_DTPOFF32 ld"), "{text}");
    // Initial exec: the GOT load the linkers relax, then the thread
    // pointer added without writing the flags.
    expect(&[
        "48 8b 1d 00 00 00 00 movq (%rip), %rbx",
        "R_X86_64_GOTTPOFF ie-0x4",
        "41 53 pushq %r11",
        "64 4c 8b 1c 25 00 00 00 00 movq %fs:0x0, %r11",
        "4a 8d 1c 1b leaq (%rbx,%r11), %rbx",
        "41 5b popq %r11",
    ]);
    // Local exec and the thread pointer.
    expect(&[
        "64 48 8b 1c 25 00 00 00 00 movq %fs:0x0, %rbx",
        "48 8d 9b 00 00 00 00 leaq (%rbx), %rbx",
        "R_X86_64_TPOFF32 .Lle$local",
    ]);
    // The operand form keeps the flags in five bytes and rebuilds them.
    for set in ["seto", "setb", "sete", "sets", "setp"] {
        assert!(text.contains(set), "{set} in\n{text}");
    }
    assert!(text.contains("rolq %r"), "{text}");
}

/// `spec/codegen.md` §5.6.11: `llvm.threadlocal.address` and
/// `llvm.thread.pointer` compile in every relocation mode.
#[test]
fn the_intrinsics_compile_in_every_mode() {
    let ir = "@x = thread_local global i32 1\n\
              declare ptr @llvm.threadlocal.address.p0(ptr)\n\
              declare ptr @llvm.thread.pointer()\n\
              define ptr @a() {\n  %p = call ptr @llvm.threadlocal.address.p0(ptr @x)\n  ret ptr %p\n}\n\
              define ptr @t() {\n  %p = call ptr @llvm.thread.pointer()\n  ret ptr %p\n}\n";
    for mode in [Mode::Static, Mode::Pie, Mode::Shared] {
        assert!(compile(ir, mode).is_ok(), "{mode:?}");
    }
}
