//! WP-314, design 0008's I10: exceptions on i686 (`spec/codegen-x86.md`
//! §14). C++ programs of `tests/data/i386/eh` compiled by clang 19 and
//! 22 (`--target=i686-linux-gnu`) and the hand-written `eh/live.ll`, built
//! by `llc`, by hod and by hod under `hod_cg::Stress::ALL`; static
//! (`-fno-pic`, a non-PIE program), PIE (`-fPIE`) and, at CPU `i586` (no
//! `cmov`), static again; a throw across a shared library boundary
//! (`eh/lib.cpp` as a library, `eh/main.cpp` as a PIE, with a dynamic
//! thread-local variable in the catching frame). Every link by GNU ld, lld
//! and mold, every run under `qemu-i386` equal to `llc`'s.
//!
//! hod's `.eh_frame` passes `llvm-dwarfdump --verify` (19 and 22), and is
//! compared with `llc`'s after decoding: the same CIEs, the same number of
//! FDEs and of FDEs with an LSDA, and per FDE the same rules once the
//! advances and the rows of `ebx`, `esi`, `edi` are left out (hod saves
//! all three in every frame, `llc` those it uses); the relocation kinds of
//! the tables (`R_386_PC32` to the `DW.ref` slot, to the type-info stubs,
//! to the LSDAs and the code) and the slot's shape are `llc`'s under PIC.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers, tools_required};

mod i386_support;
use i386_support::{back_end_object, checked, clang_module, llc_object, llvm_tool, run, tool};

fn data(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/eh").join(path)
}

/// The tools every test here needs.
struct Tools {
    gcc: PathBuf,
    qemu: PathBuf,
    linkers: Vec<(&'static str, PathBuf)>,
}

fn tools() -> Option<Tools> {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        assert!(!tools_required(), "i686 gcc or qemu-i386 missing (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: no i686 gcc or qemu-i386; skipping");
        return None;
    };
    let linkers = linkers::present(target);
    for name in ["bfd", "lld", "mold"] {
        assert!(
            !tools_required() || linkers.iter().any(|(n, _)| *n == name),
            "linker {name} missing (HOD_REQUIRE_TOOLS=1)"
        );
    }
    Some(Tools { gcc, qemu, linkers })
}

impl Tools {
    /// Links the C++ `inputs` with `flags` by `linker` into `out`.
    fn link(
        &self,
        scratch: &Scratch,
        linker: &(&str, PathBuf),
        flags: &[&str],
        inputs: &[&Path],
        out: &Path,
    ) {
        let mut command = Command::new(&self.gcc);
        let driver: Vec<OsString> = linkers::driver_flags(scratch.path(), linker.0, &linker.1);
        command.args(driver).args(flags).args(inputs).args(["-lstdc++", "-lm"]);
        command.arg("-o").arg(out);
        checked(&format!("{} link of {}", linker.0, out.display()), &mut command);
    }
}

/// hod's object of `ll` under `model`, by the back end, stressed or not.
fn hod_object(ll: &Path, model: hod_cg::RelocModel, stress: bool) -> Vec<u8> {
    let stress = if stress { hod_cg::Stress::ALL } else { hod_cg::Stress::default() };
    let options = hod_cg::Options { reloc_model: model, stress, ..hod_cg::Options::default() };
    back_end_object(&std::fs::read(ll).unwrap(), &options)
}

/// The three objects of `ll`: `llc`'s, hod's, hod's stressed.
fn objects(
    scratch: &Scratch,
    llc: &Path,
    ll: &Path,
    pic: bool,
    label: &str,
) -> [(&'static str, PathBuf); 3] {
    let (flag, model) = if pic {
        ("-relocation-model=pic", hod_cg::RelocModel::Pic)
    } else {
        ("-relocation-model=static", hod_cg::RelocModel::Static)
    };
    let llc_o = llc_object(scratch, llc, ll, &[flag], &format!("{label}-llc"));
    let hod_o = scratch.write(&format!("{label}-hod.o"), hod_object(ll, model, false));
    let stress_o = scratch.write(&format!("{label}-stress.o"), hod_object(ll, model, true));
    [("llc", llc_o), ("hod", hod_o), ("stress", stress_o)]
}

/// One run: who built it, and its status and output.
type Run<'a> = (&'a str, (Option<i32>, Vec<u8>));

fn all_equal(results: &[Run<'_>], what: &str) {
    let (_, reference) = &results[0];
    for (who, result) in &results[1..] {
        assert_eq!(
            reference,
            result,
            "{what}: {who}'s run differs from llc's\nllc: {}\n{who}: {}",
            String::from_utf8_lossy(&reference.1),
            String::from_utf8_lossy(&result.1)
        );
    }
}

/// The variants of a module: a label, clang's flags, `llc`/hod under PIC.
const VARIANTS: [(&str, &[&str], bool); 3] = [
    ("static", &["-fno-pic"], false),
    ("pie", &["-fPIE"], true),
    ("i586", &["-fno-pic", "-march=i586"], false),
];

/// The C++ programs, each with its clang flags.
const PROGRAMS: [(&str, &[&str]); 2] = [("basic.cpp", &["-std=c++14"]), ("terminate.cpp", &[])];

/// `eh/callconv.ll` with its functions at `cpu`.
fn callconv_at(scratch: &Scratch, cpu: &str) -> PathBuf {
    let text = std::fs::read_to_string(data("callconv.ll")).unwrap();
    let plain = "attributes #0 = { noinline }";
    assert!(text.contains(plain));
    let at = format!("attributes #0 = {{ noinline \"target-cpu\"=\"{cpu}\" }}");
    scratch.write(&format!("callconv-{cpu}.ll"), text.replace(plain, &at).as_bytes())
}

/// What `eh/callconv.ll` prints (checked on `llc`'s run, so that the
/// shapes are what they claim): per `%v`, `calls`' result (negative where
/// a callee threw, the callee by its number), then `x87_live` (0: both
/// quotients infinite, the products rounded before the `invoke`; 16 at
/// `%v` = 4, the pad) and `x87_pad` (19 at `%v` = 5: both quotients
/// finite, computed at 80 bits within the pad).
const CALLCONV_OUTPUT: &str = "0: 1009 0 0\n1: -1114 0 0\n2: -1228 0 0\n3: -1342 0 0\n\
                               4: 1553 16 0\n5: 1754 0 19\n";

/// §14: the programs, `live.ll` and `callconv.ll`, every variant, every
/// linker.
#[test]
fn exceptions_run_as_llc_s() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-eh");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            assert!(!tools_required(), "LLVM {major} missing (HOD_REQUIRE_TOOLS=1)");
            continue;
        };
        let mut modules: Vec<(String, PathBuf, bool)> = Vec::new();
        for (program, extra) in PROGRAMS {
            for (variant, flags, pic) in VARIANTS {
                let label = format!("{program}-{major}-{variant}");
                let flags: Vec<&str> = flags.iter().chain(extra).copied().collect();
                let ll = clang_module(&scratch, &clang, &data(program), &flags, &label);
                modules.push((label, ll, pic));
            }
        }
        modules.push((format!("live-{major}-static"), data("live.ll"), false));
        modules.push((format!("live-{major}-pie"), data("live.ll"), true));
        let cmov = callconv_at(&scratch, "pentiumpro");
        for (variant, ll) in [("generic", data("callconv.ll")), ("cmov", cmov)] {
            modules.push((format!("callconv-{major}-{variant}-static"), ll.clone(), false));
            modules.push((format!("callconv-{major}-{variant}-pie"), ll, true));
        }
        for (label, ll, pic) in &modules {
            let objects = objects(&scratch, &llc, ll, *pic, label);
            for linker in &tools.linkers {
                let mut results = Vec::new();
                for (who, object) in &objects {
                    let exe = scratch.join(format!("{label}-{who}-{}", linker.0));
                    let flags: &[&str] = if *pic { &["-pie"] } else { &["-no-pie"] };
                    tools.link(&scratch, linker, flags, &[object], &exe);
                    results.push((*who, run(&tools.qemu, &exe)));
                }
                all_equal(&results, &format!("{label} ({})", linker.0));
                if label.starts_with("callconv") {
                    assert_eq!(
                        String::from_utf8_lossy(&results[0].1.1),
                        CALLCONV_OUTPUT,
                        "{label}: llc's run"
                    );
                }
                runs += 1;
            }
        }
    }
    eprintln!("i386 EH: {runs} program runs (llc, hod, hod stressed) equal");
}

/// §14: a throw across a shared library boundary, both ways, with a
/// general-dynamic thread-local variable in the catching frame; each
/// compiler's library with each compiler's program.
#[test]
fn exceptions_cross_a_shared_library() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-eh-so");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        let lib_ll = clang_module(&scratch, &clang, &data("lib.cpp"), &["-fPIC"], "lib");
        let main_ll = clang_module(&scratch, &clang, &data("main.cpp"), &["-fPIE"], "main");
        let libs = objects(&scratch, &llc, &lib_ll, true, &format!("lib-{major}"));
        let mains = objects(&scratch, &llc, &main_ll, true, &format!("main-{major}"));
        for linker in &tools.linkers {
            let mut results = Vec::new();
            for ((who, lib_o), (_, main_o)) in libs.iter().zip(&mains) {
                let dir = scratch.join(format!("{major}-{who}-{}", linker.0));
                std::fs::create_dir_all(&dir).unwrap();
                let lib = dir.join("libeh.so");
                tools.link(&scratch, linker, &["-shared"], &[lib_o], &lib);
                let exe = dir.join("main");
                let rpath = format!("-Wl,-rpath,{}", dir.display());
                tools.link(&scratch, linker, &["-pie", &rpath], &[main_o, &lib], &exe);
                results.push((*who, run(&tools.qemu, &exe)));
            }
            all_equal(&results, &format!("shared library {major} ({})", linker.0));
            runs += 1;
        }
    }
    eprintln!("i386 EH across a shared library: {runs} runs equal");
}

/// `llvm-dwarfdump --eh-frame` of `object`, its entries split: each CIE
/// or FDE as its header line's kind and its instruction lines.
fn eh_entries(dwarfdump: &Path, object: &Path) -> Vec<(String, Vec<String>)> {
    let outcome = checked("llvm-dwarfdump", Command::new(dwarfdump).arg("--eh-frame").arg(object));
    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    for line in String::from_utf8_lossy(&outcome.stdout).lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 4 && (fields[3] == "CIE" || fields[3] == "FDE") {
            entries.push((fields[3].to_owned(), Vec::new()));
        } else if let Some((_, lines)) = entries.last_mut() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with("0x") {
                lines.push(line.to_owned());
            }
        }
    }
    entries
}

/// An FDE's rules up to its first exit (`def_cfa: ESP +4`) without the
/// advances, the `nop`s and the rows of `ebx`, `esi` and `edi`; whether
/// it has an LSDA. The rows after the first exit differ by design: `llc`
/// writes an exit per `ret` and restores the `ebp` rule after each for
/// the code that follows (`def_cfa: EBP +8`), hod has one exit at the
/// end of the function (`spec/codegen-x86.md` §3), also in a function
/// that never returns.
fn fde_shape(lines: &[String]) -> (Vec<String>, bool) {
    let lsda = lines.iter().any(|l| l.starts_with("LSDA Address"));
    let rules = lines
        .iter()
        .filter(|l| l.starts_with("DW_CFA_"))
        .filter(|l| !l.starts_with("DW_CFA_advance_loc") && !l.starts_with("DW_CFA_nop"))
        .filter(|l| !["EBX", "ESI", "EDI"].iter().any(|r| l.contains(&format!(" {r} "))))
        .take_while(|l| !l.starts_with("DW_CFA_def_cfa: ESP +4"))
        .cloned()
        .collect();
    (rules, lsda)
}

/// A summary of `object`'s `.eh_frame`: the CIEs (without the
/// personality's unrelocated address) and the multiset of FDE shapes.
fn eh_summary(dwarfdump: &Path, object: &Path) -> (Vec<Vec<String>>, BTreeMap<String, usize>) {
    let mut cies = Vec::new();
    let mut fdes = BTreeMap::new();
    for (kind, lines) in eh_entries(dwarfdump, object) {
        if kind == "CIE" {
            cies.push(
                lines.into_iter().filter(|l| !l.starts_with("Personality Address")).collect(),
            );
        } else {
            let (rules, lsda) = fde_shape(&lines);
            *fdes.entry(format!("lsda={lsda} {}", rules.join("; "))).or_insert(0) += 1;
        }
    }
    (cies, fdes)
}

/// The relocation kinds and targets of `object`'s unwind sections and of
/// the `DW.ref` slot, local section symbols by kind only.
fn table_relocations(readelf: &Path, object: &Path) -> BTreeMap<String, Vec<String>> {
    let outcome = checked("readelf -rW", Command::new(readelf).arg("-rW").arg(object));
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut section = None;
    for line in String::from_utf8_lossy(&outcome.stdout).lines() {
        if let Some(rest) = line.strip_prefix("Relocation section '") {
            let name = rest.split('\'').next().unwrap_or("").to_owned();
            let keep = name.contains("eh_frame")
                || name.contains("gcc_except_table")
                || name.contains("DW.ref");
            section = keep.then_some(name);
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if let Some(name) = &section
            && fields.len() >= 5
            && fields[2].starts_with("R_386_")
        {
            let sym = fields[4];
            let sym = if sym.starts_with('.') { ".<section>" } else { sym };
            out.entry(name.clone()).or_default().push(format!("{} {sym}", fields[2]));
        }
    }
    for kinds in out.values_mut() {
        kinds.sort();
        kinds.dedup();
    }
    out
}

/// §14: hod's tables verify, and decode to `llc`'s shapes.
#[test]
fn unwind_tables_verify_and_decode_as_llc_s() {
    let readelf = tool("readelf");
    let mut compared = 0;
    for major in LLVM_MAJORS {
        let scratch = Scratch::new("i386-eh-tables");
        let (Some(clang), Some(llc), Some(dwarfdump)) = (
            llvm_tool(major, "clang"),
            llvm_tool(major, "llc"),
            llvm_tool(major, "llvm-dwarfdump"),
        ) else {
            assert!(!tools_required(), "LLVM {major} missing (HOD_REQUIRE_TOOLS=1)");
            continue;
        };
        let basic = clang_module(&scratch, &clang, &data("basic.cpp"), &["-std=c++14"], "basic");
        let lib = clang_module(&scratch, &clang, &data("lib.cpp"), &["-fPIC"], "lib");
        for (label, ll) in [("basic", basic), ("lib", lib), ("live", data("live.ll"))] {
            let [(_, theirs), (_, ours), (_, stressed)] =
                objects(&scratch, &llc, &ll, true, &format!("{label}-{major}"));
            for object in [&ours, &stressed] {
                let verify = Command::new(&dwarfdump).arg("--verify").arg(object).output().unwrap();
                let text = String::from_utf8_lossy(&verify.stdout);
                assert!(verify.status.success() && text.contains("No errors."), "{label}: {text}");
            }
            // `live.ll` has no `"frame-pointer"="all"`: `llc` keeps the
            // CFA on `esp` there, hod always on `ebp` (§3).
            if label != "live" {
                let (cies, fdes) = eh_summary(&dwarfdump, &theirs);
                assert_eq!(eh_summary(&dwarfdump, &ours), (cies.clone(), fdes.clone()), "{label}");
                assert_eq!(eh_summary(&dwarfdump, &stressed), (cies, fdes), "{label} (stress)");
            }
            if let Some(readelf) = &readelf {
                let theirs = table_relocations(readelf, &theirs);
                assert_eq!(table_relocations(readelf, &ours), theirs, "{label}: relocations");
            }
            compared += 1;
        }
    }
    eprintln!("i386 EH: {compared} objects' tables verify and decode as llc's");
}

/// An `invoke` of an `x86_stdcallcc` or `x86_fastcallcc` callee (callee
/// pop) is refused by the convention's name, as a `call` of one is
/// (`spec/abi-x86.md`); `callconv.ll` runs the callee pops hod has: an
/// `sret` pointer.
#[test]
fn invoke_of_stdcall_and_fastcall_is_refused_by_name() {
    for cc in ["x86_stdcallcc", "x86_fastcallcc"] {
        let text = format!(
            "target triple = \"{}\"\n\
             declare i32 @__gxx_personality_v0(...)\n\
             declare {cc} void @s(i32)\n\
             define void @f() personality ptr @__gxx_personality_v0 {{\n\
             \x20 invoke {cc} void @s(i32 1) to label %ok unwind label %pad\n\
             ok:\n  ret void\n\
             pad:\n  %lp = landingpad {{ ptr, i32 }} cleanup\n\
             \x20 resume {{ ptr, i32 }} %lp\n}}\n",
            i386_support::TRIPLE
        );
        let options = hod::Options {
            output_kind: hod::OutputKind::Object,
            triple: Some(i386_support::TRIPLE.into()),
            ..hod::Options::default()
        };
        let err = hod::compile(text.as_bytes(), &options).expect_err("refused");
        assert!(err.to_string().contains(&format!("cc:{cc}")), "{cc}: {err}");
    }
}

/// A section header of a little-endian ELF32 object.
#[derive(Clone, Copy)]
struct Shdr {
    name: u32,
    kind: u32,
    offset: usize,
    size: usize,
    info: u32,
}

/// The parts of an ELF32 relocatable object the LSDA test reads.
struct Elf<'a> {
    data: &'a [u8],
    sections: Vec<Shdr>,
    /// Each symbol: name, value, section index, whether `STT_SECTION`.
    symbols: Vec<(String, u32, u16, bool)>,
}

fn le32(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn le16(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(data[at..at + 2].try_into().unwrap())
}

fn cstr(data: &[u8], at: usize) -> String {
    let end = data[at..].iter().position(|&b| b == 0).unwrap();
    String::from_utf8_lossy(&data[at..at + end]).into_owned()
}

impl<'a> Elf<'a> {
    fn parse(data: &'a [u8]) -> Self {
        assert_eq!(&data[..6], b"\x7fELF\x01\x01", "an ELF32 little-endian object");
        let shoff = le32(data, 0x20) as usize;
        let count = usize::from(le16(data, 0x30));
        let sections: Vec<Shdr> = (0..count)
            .map(|i| {
                let h = shoff + 40 * i;
                Shdr {
                    name: le32(data, h),
                    kind: le32(data, h + 4),
                    offset: le32(data, h + 16) as usize,
                    size: le32(data, h + 20) as usize,
                    info: le32(data, h + 28),
                }
            })
            .collect();
        let mut elf = Elf { data, sections, symbols: Vec::new() };
        let (symtab, strtab) = {
            let i = elf.sections.iter().position(|s| s.kind == 2).unwrap();
            let h = shoff + 40 * i;
            (elf.sections[i], elf.sections[le32(data, h + 24) as usize])
        };
        for k in 0..symtab.size / 16 {
            let e = symtab.offset + 16 * k;
            let name = cstr(data, strtab.offset + le32(data, e) as usize);
            let section = data[e + 12] & 0xf == 3;
            elf.symbols.push((name, le32(data, e + 4), le16(data, e + 14), section));
        }
        elf
    }

    fn name(&self, index: usize) -> String {
        let shstrndx = usize::from(le16(self.data, 0x32));
        cstr(self.data, self.sections[shstrndx].offset + self.sections[index].name as usize)
    }

    fn bytes(&self, index: usize) -> &'a [u8] {
        let s = self.sections[index];
        &self.data[s.offset..s.offset + s.size]
    }

    /// The `SHT_REL` entries applying to section `index`: offset,
    /// symbol, implicit addend.
    fn rels(&self, index: usize) -> Vec<(usize, usize, u32)> {
        let mut out = Vec::new();
        for s in self.sections.iter().filter(|s| s.kind == 9 && s.info as usize == index) {
            for k in 0..s.size / 8 {
                let offset = le32(self.data, s.offset + 8 * k) as usize;
                let sym = (le32(self.data, s.offset + 8 * k + 4) >> 8) as usize;
                out.push((offset, sym, le32(self.bytes(index), offset)));
            }
        }
        out.sort_unstable();
        out
    }

    /// What the 4-byte field at `offset` of section `index` points to: a
    /// named symbol, or through a local slot (a section symbol plus the
    /// implicit addend) the symbol of the slot's own relocation;
    /// `DW.ref.` stripped; `null` for an unrelocated zero.
    fn target(&self, index: usize, offset: usize) -> String {
        let Some(&(_, sym, addend)) = self.rels(index).iter().find(|r| r.0 == offset) else {
            assert_eq!(le32(self.bytes(index), offset), 0, "an unrelocated type entry");
            return "null".to_owned();
        };
        let (name, value, shndx, section) = &self.symbols[sym];
        if *section || name.is_empty() {
            return self.target(usize::from(*shndx), (value + addend) as usize);
        }
        name.strip_prefix("DW.ref.").unwrap_or(name).to_owned()
    }
}

fn uleb(data: &[u8], at: &mut usize) -> u64 {
    let (mut value, mut shift) = (0u64, 0);
    loop {
        let byte = data[*at];
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            return value;
        }
    }
}

fn sleb(data: &[u8], at: &mut usize) -> i64 {
    let (mut value, mut shift) = (0i64, 0);
    loop {
        let byte = data[*at];
        *at += 1;
        value |= i64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 64 && byte & 0x40 != 0 {
                value |= -1 << shift;
            }
            return value;
        }
    }
}

/// One call site of an LSDA: whether it has a landing pad, and the
/// action chain. Not which pad: where several `invoke`s unwind to one IR
/// pad, `llc` gives them one landing address and hod one per `invoke`
/// (measured on `basic.cpp`), so pad identity is compared by the runs.
type Site = (bool, Vec<String>);

/// Every LSDA of `object` in FDE order, decoded: the encodings, the call
/// sites (consecutive equal ones merged: where `llc` and hod place the
/// calls differs, what each range does may not) and the type table as
/// the catch clauses name it. Asserts that the ranges are ordered and
/// disjoint.
fn lsdas(object: &[u8]) -> Vec<(String, Vec<Site>)> {
    let elf = Elf::parse(object);
    let eh = (0..elf.sections.len()).find(|&i| elf.name(i) == ".eh_frame").unwrap();
    let mut out = Vec::new();
    for (_, sym, addend) in elf.rels(eh) {
        let (_, value, shndx, _) = elf.symbols[sym];
        let index = usize::from(shndx);
        if !elf.name(index).starts_with(".gcc_except_table") {
            continue;
        }
        let data = elf.bytes(index);
        let mut at = (value + addend) as usize;
        let (lpstart, ttype) = (data[at], data[at + 1]);
        at += 2;
        let ttype_base = if ttype == 0xff {
            None
        } else {
            let off = uleb(data, &mut at) as usize;
            Some(at + off)
        };
        let cs = data[at];
        at += 1;
        let cs_len = uleb(data, &mut at) as usize;
        let actions = at + cs_len;
        let mut sites: Vec<Site> = Vec::new();
        let mut end = 0;
        while at < actions {
            let (start, len, pad, action) = (
                uleb(data, &mut at),
                uleb(data, &mut at),
                uleb(data, &mut at),
                uleb(data, &mut at),
            );
            assert!(start >= end, "call-site ranges out of order or overlapping");
            end = start + len;
            let pad = pad != 0;
            let mut chain = Vec::new();
            let mut next = action as usize;
            while next != 0 {
                let mut a = actions + next - 1;
                let filter = sleb(data, &mut a);
                let link = a;
                let disp = sleb(data, &mut a);
                chain.push(match filter {
                    0 => "cleanup".to_owned(),
                    f if f > 0 => {
                        let base = ttype_base.expect("a catch without a type table");
                        format!("catch {}", elf.target(index, base - 4 * f as usize))
                    }
                    f => {
                        let base = ttype_base.expect("a filter without a type table");
                        let mut p = base + (-f - 1) as usize;
                        let mut types = Vec::new();
                        loop {
                            let t = uleb(data, &mut p) as usize;
                            if t == 0 {
                                break;
                            }
                            types.push(elf.target(index, base - 4 * t));
                        }
                        format!("filter [{}]", types.join(", "))
                    }
                });
                next =
                    if disp == 0 { 0 } else { (link as i64 + disp - actions as i64 + 1) as usize };
            }
            if sites.last() != Some(&(pad, chain.clone())) {
                sites.push((pad, chain));
            }
        }
        out.push((format!("lpstart {lpstart:#x} ttype {ttype:#x} cs {cs:#x}"), sites));
    }
    out
}

/// §14: the LSDAs of hod's objects decode as `llc`'s (static and PIC:
/// hod writes `llc`'s PIC encodings in every model, so the static
/// objects are compared after decoding, as the PIC ones are).
#[test]
fn lsdas_decode_as_llc_s() {
    let mut compared = 0;
    let mut tables = 0;
    // The kinds of clause `llc`'s tables have: each must be decoded.
    let mut clauses: BTreeMap<String, usize> = BTreeMap::new();
    for major in LLVM_MAJORS {
        let scratch = Scratch::new("i386-eh-lsda");
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            assert!(!tools_required(), "LLVM {major} missing (HOD_REQUIRE_TOOLS=1)");
            continue;
        };
        let basic = clang_module(&scratch, &clang, &data("basic.cpp"), &["-std=c++14"], "basic");
        let modules =
            [("basic", basic), ("live", data("live.ll")), ("callconv", data("callconv.ll"))];
        for (label, ll) in modules {
            for pic in [false, true] {
                let objects = objects(&scratch, &llc, &ll, pic, &format!("{label}-{major}-{pic}"));
                let read = |p: &Path| lsdas(&std::fs::read(p).unwrap());
                let theirs = read(&objects[0].1);
                assert!(!theirs.is_empty(), "{label}: llc wrote no LSDA");
                for (_, sites) in &theirs {
                    for (_, chain) in sites {
                        for action in chain {
                            let kind = action.split(' ').next().unwrap().to_owned();
                            *clauses.entry(kind).or_insert(0) += 1;
                        }
                    }
                }
                for (who, object) in &objects[1..] {
                    let ours = read(object);
                    assert_eq!(ours.len(), theirs.len(), "{label} {who} pic={pic}: LSDA count");
                    for (k, (o, t)) in ours.iter().zip(&theirs).enumerate() {
                        // The encodings are `llc`'s PIC ones in every model.
                        if pic {
                            assert_eq!(o.0, t.0, "{label} {who}: LSDA {k} encodings");
                        }
                        assert_eq!(o.1, t.1, "{label} {who} pic={pic}: LSDA {k} call sites");
                    }
                    tables += ours.len();
                }
                compared += 1;
            }
        }
    }
    for kind in ["catch", "cleanup", "filter"] {
        assert!(clauses.contains_key(kind) || !tools_required(), "no {kind} clause: {clauses:?}");
    }
    eprintln!("i386 EH: {compared} module builds, {tables} LSDAs decode as llc's ({clauses:?})");
}
