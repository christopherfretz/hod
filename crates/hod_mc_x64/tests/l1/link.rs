//! The TLS access sequences through the linkers: `spec/mc-x64.md`
//! "Thread-local storage".
//!
//! For each model the sequence is built twice: from hod's bytes, with a
//! `.reloc` at each of hod's sites (its offset, its addend), and from the
//! assembly text a compiler writes for it; their code and relocations must
//! be equal, at every LLVM major. Both objects are linked by GNU
//! ld, lld and mold into a static executable, a position-independent
//! executable (where the linkers relax every model to local or initial
//! exec, rewriting hod's bytes) and a shared object (where they relax
//! nothing and emit dynamic TLS relocations); the outputs must be
//! identical. The programs are not run: this host cannot execute x86-64
//! code. A control shows that the comparison sees the padding: the general
//! dynamic pair without its prefixes is refused by GNU ld and rewritten
//! into different bytes by lld and mold.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use hod_mc_x64::*;
use hod_testutil::{run, tool};

use crate::harness::Tools;

/// One instruction: hod's encoding, the relocation at its site (type and
/// symbol), and the text for the same instruction and relocation.
type Item = (Inst, Option<(&'static str, &'static str)>, String);

fn field(m: Mem) -> Mem {
    m.with_abs32().expect("not RIP-relative")
}

/// `mov rax, qword ptr fs:[0]`: the thread pointer.
fn thread_pointer() -> Item {
    let fs0 = Mem::abs(0).with_seg(Seg::Fs);
    (mov_rm(Size::Q, Reg::Rax, fs0), None, "mov rax, qword ptr fs:[0]".into())
}

fn general_dynamic() -> Vec<Item> {
    vec![
        (tls_gd_lea(), Some(("TLSGD", "x")), "data16 lea rdi, [rip + x@TLSGD]".into()),
        (
            tls_gd_call(),
            Some(("PLT32", "__tls_get_addr")),
            "data16 data16 rex64 call __tls_get_addr@PLT".into(),
        ),
    ]
}

fn local_dynamic() -> Vec<Item> {
    let rax = Mem::base(Reg::Rax, 0);
    vec![
        (
            lea(Size::Q, Reg::Rdi, Mem::rip(0)),
            Some(("TLSLD", "x")),
            "lea rdi, [rip + x@TLSLD]".into(),
        ),
        (call(0), Some(("PLT32", "__tls_get_addr")), "call __tls_get_addr@PLT".into()),
        (
            mov_rm(Size::D, Reg::Rcx, field(rax)),
            Some(("DTPOFF32", "x")),
            "mov ecx, dword ptr [rax + x@DTPOFF]".into(),
        ),
        (
            lea(Size::Q, Reg::Rax, field(rax)),
            Some(("DTPOFF32", "x")),
            "lea rax, [rax + x@DTPOFF]".into(),
        ),
    ]
}

fn initial_exec() -> Vec<Item> {
    let mut out = Vec::new();
    for n in [0, 3, 8, 12, 13, 15] {
        let r = crate::text::r(n);
        let q = crate::text::reg(Size::Q, n);
        out.push((
            mov_rm(Size::Q, r, Mem::rip(0)),
            Some(("GOTTPOFF", "x")),
            format!("mov {q}, qword ptr [rip + x@GOTTPOFF]"),
        ));
        out.push((
            add_rm(Size::Q, r, Mem::rip(0)),
            Some(("GOTTPOFF", "x")),
            format!("add {q}, qword ptr [rip + x@GOTTPOFF]"),
        ));
    }
    out.push(thread_pointer());
    out.push((
        add_rm(Size::Q, Reg::Rax, Mem::rip(0)),
        Some(("GOTTPOFF", "x")),
        "add rax, qword ptr [rip + x@GOTTPOFF]".into(),
    ));
    out
}

fn local_exec() -> Vec<Item> {
    let fs = field(Mem::abs(0).with_seg(Seg::Fs));
    vec![
        thread_pointer(),
        (
            lea(Size::Q, Reg::Rax, field(Mem::base(Reg::Rax, 0))),
            Some(("TPOFF32", "x")),
            "lea rax, [rax + x@TPOFF]".into(),
        ),
        (
            mov_rm(Size::D, Reg::Rdx, fs),
            Some(("TPOFF32", "x")),
            "mov edx, dword ptr fs:[x@TPOFF]".into(),
        ),
        (mov_mi(Size::D, fs, 9), Some(("TPOFF32", "x")), "mov dword ptr fs:[x@TPOFF], 9".into()),
    ]
}

/// A thread-local `x`, then `_start` with `items`, one label per
/// instruction.
fn source(items: &[Item], from_hod: bool) -> String {
    let mut s = String::from(
        ".section .tdata,\"awT\",@progbits\n.globl x\n.type x,@object\n.p2align 3\n\
         x: .quad 7\n.size x, 8\n.text\n.globl _start\n.type _start,@function\n_start:\n",
    );
    for (i, (inst, reloc, text)) in items.iter().enumerate() {
        if from_hod {
            let bytes: Vec<String> = inst.bytes().iter().map(|b| format!("{b:#04x}")).collect();
            writeln!(s, "h{i}: .byte {}", bytes.join(",")).unwrap();
            if let Some((kind, symbol)) = reloc {
                let site = inst.site().expect("a relocated instruction has a site");
                writeln!(
                    s,
                    ".reloc h{i}+{}, R_X86_64_{kind}, {symbol}{:+}",
                    site.offset, site.addend
                )
                .unwrap();
            }
        } else {
            writeln!(s, "h{i}: {text}").unwrap();
        }
    }
    s.push_str("ret\n");
    s
}

/// `__tls_get_addr`, for the executables (where no call to it remains).
const STUB: &str = ".text\n.globl __tls_get_addr\n.type __tls_get_addr,@function\n\
                    __tls_get_addr: ret\n";

#[derive(Debug)]
struct Linker {
    name: &'static str,
    path: PathBuf,
}

/// GNU ld for x86-64 (the host's on an x86-64 host, else the cross
/// linker), lld and mold. A missing one is skipped with a note, or a
/// failure under `HOD_REQUIRE_TOOLS=1`.
fn linkers() -> Vec<Linker> {
    let bfd = if cfg!(target_arch = "x86_64") { "ld.bfd" } else { "x86_64-linux-gnu-ld.bfd" };
    [("bfd", bfd), ("lld", "ld.lld"), ("mold", "mold")]
        .into_iter()
        .filter_map(|(name, program)| Some(Linker { name, path: tool(program)? }))
        .collect()
}

/// How an object is linked.
const MODES: [(&str, &[&str]); 3] = [
    ("static", &["-static", "-e", "_start"]),
    ("pie", &["-pie", "-e", "_start"]),
    ("shared", &["-shared"]),
];

/// Links `objects` and returns the output, or the linker's diagnostics.
fn link(linker: &Linker, flags: &[&str], objects: &[&Path], out: &Path) -> Result<Vec<u8>, String> {
    let mut args: Vec<&std::ffi::OsStr> =
        ["-m", "elf_x86_64", "--fatal-warnings"].iter().map(AsRef::as_ref).collect();
    args.extend(flags.iter().map(|s| std::ffi::OsStr::new(*s)));
    args.extend(["-o".as_ref(), out.as_os_str()]);
    args.extend(objects.iter().map(|p| p.as_os_str()));
    let outcome = run(&linker.path, args);
    assert!(!outcome.timed_out(), "{} timed out", linker.name);
    if outcome.success() && outcome.stderr_text().is_empty() {
        Ok(std::fs::read(out).expect("the linker wrote its output"))
    } else {
        Err(outcome.stderr_text())
    }
}

/// The relocations of an object as `llvm-objdump -r` prints them, in
/// offset order.
fn relocations(tools: &Tools, obj: &Path) -> Vec<String> {
    let outcome = run(tools.objdump(), ["-r".as_ref(), obj.as_os_str()]);
    assert!(outcome.success(), "llvm-objdump -r failed: {}", outcome.stderr_text());
    let mut lines: Vec<String> = outcome
        .stdout_text()
        .lines()
        .filter(|l| l.contains("R_X86_64_"))
        .map(String::from)
        .collect();
    lines.sort();
    lines
}

/// The dynamic relocation types of a linked file.
fn dynamic_relocations(tools: &Tools, file: &Path) -> String {
    let outcome = run(tools.objdump(), ["-R".as_ref(), file.as_os_str()]);
    assert!(outcome.success(), "llvm-objdump -R failed: {}", outcome.stderr_text());
    outcome.stdout_text()
}

/// `len` bytes of a linked file's code at label `h0`: its address from the
/// symbol table, less `.text`'s address from the section headers, into
/// `.text`'s contents.
fn h0_bytes(tools: &Tools, file: &Path, len: usize) -> Vec<u8> {
    let address = tools.symbols(file)["h0"];
    let outcome = run(tools.objdump(), ["-h".as_ref(), file.as_os_str()]);
    let headers = outcome.stdout_text();
    let text = headers
        .lines()
        .find(|l| l.split_whitespace().nth(1) == Some(".text"))
        .expect("a .text section");
    let vma = text.split_whitespace().nth(3).expect("a VMA column");
    let vma = u64::from_str_radix(vma, 16).expect("a VMA");
    let at = usize::try_from(address - vma).expect("an offset");
    tools.text_bytes(file)[at..at + len].to_vec()
}

struct Built {
    hod: PathBuf,
    llvm: PathBuf,
}

/// Both objects for `items`, under the same file name in two directories
/// (the name is in a linked file's symbol table).
fn build(tools: &Tools, label: &str, items: &[Item]) -> Built {
    for dir in ["hod", "llvm"] {
        std::fs::create_dir_all(tools.scratch().join(dir)).expect("a scratch directory");
    }
    let hod = tools.assemble_object(&format!("hod/{label}"), &source(items, true));
    let llvm = tools.assemble_object(&format!("llvm/{label}"), &source(items, false));
    let built = Built {
        hod: hod.expect("llvm-mc assembles hod's bytes"),
        llvm: llvm.expect("llvm-mc assembles the text"),
    };
    assert_eq!(
        tools.text_bytes(&built.hod),
        tools.text_bytes(&built.llvm),
        "LLVM {}: {label}: hod's bytes",
        tools.major
    );
    assert_eq!(
        relocations(tools, &built.hod),
        relocations(tools, &built.llvm),
        "LLVM {}: {label}: hod's sites and addends",
        tools.major
    );
    built
}

#[test]
fn tls_sequences_link_as_llvms() {
    let linkers = linkers();
    let models: [(&str, Vec<Item>); 4] = [
        ("gd", general_dynamic()),
        ("ld", local_dynamic()),
        ("ie", initial_exec()),
        ("le", local_exec()),
    ];
    // The objects are compared at every major; the links, which take most
    // of the time, are made from the newest major's objects.
    for (major_index, tools) in Tools::all().into_iter().enumerate() {
        let stub = tools.assemble_object("stub", STUB).expect("the stub assembles");
        let mut linked = 0;
        for (model, items) in &models {
            let built = build(&tools, model, items);
            if major_index > 0 {
                continue;
            }
            // The first instruction up to its field: its opcode bytes.
            let head = items[0].0.site().map_or(items[0].0.len(), |site| site.offset);
            let first = &items[0].0.bytes()[..head];
            for linker in &linkers {
                for (mode, flags) in MODES {
                    // A shared object cannot use local exec.
                    if *model == "le" && mode == "shared" {
                        continue;
                    }
                    let objects = |obj: &Path| -> Vec<PathBuf> {
                        let mut v = vec![obj.to_path_buf()];
                        if mode != "shared" {
                            v.push(stub.clone());
                        }
                        v
                    };
                    let what = format!("LLVM {} {model} {} -{mode}", tools.major, linker.name);
                    let out = |side: &str| {
                        tools.scratch().join(format!("{model}-{mode}-{}-{side}", linker.name))
                    };
                    let ours = objects(&built.hod);
                    let theirs = objects(&built.llvm);
                    let ours: Vec<&Path> = ours.iter().map(PathBuf::as_path).collect();
                    let theirs: Vec<&Path> = theirs.iter().map(PathBuf::as_path).collect();
                    let hod = link(linker, flags, &ours, &out("hod"))
                        .unwrap_or_else(|e| panic!("{what}: hod's object does not link:\n{e}"));
                    let llvm = link(linker, flags, &theirs, &out("llvm"))
                        .unwrap_or_else(|e| panic!("{what}: LLVM's object does not link:\n{e}"));
                    assert!(hod == llvm, "{what}: the linked outputs differ");
                    let at_h0 = h0_bytes(&tools, &out("hod"), first.len());
                    let dynamic = if mode == "shared" {
                        dynamic_relocations(&tools, &out("hod"))
                    } else {
                        String::new()
                    };
                    match (*model, mode) {
                        // Nothing is relaxed in a shared object: the
                        // loader resolves the module and offset.
                        ("gd" | "ld", "shared") => {
                            assert_eq!(at_h0, first, "{what}: relaxed in a shared object");
                            assert!(dynamic.contains("R_X86_64_DTPMOD64"), "{what}:\n{dynamic}");
                        }
                        ("ie", "shared") => {
                            assert!(dynamic.contains("R_X86_64_TPOFF64"), "{what}:\n{dynamic}");
                        }
                        // In an executable the dynamic models are
                        // rewritten into a thread-pointer access.
                        ("gd" | "ld", _) => assert_ne!(at_h0, first, "{what}: not relaxed"),
                        _ => {}
                    }
                    linked += 1;
                }
            }
        }
        println!(
            "l1 tls (LLVM {}): {} objects' bytes and relocations equal LLVM's; {linked} links \
             of hod's objects equal LLVM's",
            tools.major,
            models.len()
        );
    }
}

/// The control: without its padding prefixes each linker refuses the
/// general-dynamic pair (GNU ld 2.44 does) or writes something other than
/// what it writes for the padded pair (lld 22 and mold 2.37 do).
#[test]
fn tls_padding_is_what_the_linkers_see() {
    let linkers = linkers();
    let Some(tools) = Tools::find() else { return };
    let stub = tools.assemble_object("stub", STUB).expect("the stub assembles");
    let unpadded: Vec<Item> = vec![
        (lea(Size::Q, Reg::Rdi, Mem::rip(0)), Some(("TLSGD", "x")), String::new()),
        (call(0), Some(("PLT32", "__tls_get_addr")), String::new()),
    ];
    let padded = general_dynamic();
    let unpadded = tools.assemble_object("unpadded", &source(&unpadded, true)).expect("assembles");
    let padded = tools.assemble_object("padded", &source(&padded, true)).expect("assembles");
    let mut outcomes = BTreeMap::new();
    for linker in &linkers {
        let flags = MODES[0].1;
        let good_out = tools.scratch().join(format!("padded-{}", linker.name));
        link(linker, flags, &[&padded, &stub], &good_out).expect("the padded pair links");
        let good = h0_bytes(&tools, &good_out, 16);
        let bad_out = tools.scratch().join(format!("unpadded-{}", linker.name));
        let verdict = if link(linker, flags, &[&unpadded, &stub], &bad_out).is_ok() {
            let bad = h0_bytes(&tools, &bad_out, 16);
            assert_ne!(bad, good, "{}: the padding made no difference", linker.name);
            "rewritten differently"
        } else {
            "refused"
        };
        outcomes.insert(linker.name, verdict);
    }
    println!("l1 tls control: the unpadded general-dynamic pair is {outcomes:?}");
}
