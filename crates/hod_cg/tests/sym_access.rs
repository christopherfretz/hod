//! `sym_access`, `call_access` and `prefers_local_alias` against `llc`
//! (`spec/codegen.md` §7.4): a grid of symbols (variables and functions,
//! declared and defined, every linkage, visibility, `dso_local` and
//! thread-local model) is addressed and called by one function each,
//! compiled by `llc` for AArch64 and x86-64, static and PIC, with and
//! without a `PIE Level`, on every LLVM major present. The relocation
//! `llc` uses says which access it chose; `hod`'s answer for the same
//! module must be the same.

mod elfview;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use elfview::Elf;
use hod_cg::{
    Access, Options, RelocModel, TlsAccess, call_access, prefers_local_alias, sym_access,
};
use hod_ir::{
    AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19, Module, SymId,
    X86_64_LINUX_DATA_LAYOUT,
};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with, tools_required};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Arch {
    A64,
    X64,
}

/// The grid's symbols, as module text, and their names.
fn symbols() -> (String, Vec<String>) {
    let mut text = String::new();
    let mut names = Vec::new();
    for func in [false, true] {
        for defined in [false, true] {
            let linkages: &[&str] = match (defined, func) {
                (false, _) => &["", "extern_weak"],
                (true, false) => {
                    &["", "internal", "private", "weak", "linkonce_odr", "weak_odr", "common"]
                }
                (true, true) => &["", "internal", "private", "weak", "linkonce_odr", "weak_odr"],
            };
            for &linkage in linkages {
                let local = matches!(linkage, "internal" | "private");
                let visibilities: &[&str] =
                    if local { &[""] } else { &["", "hidden", "protected"] };
                for &visibility in visibilities {
                    if linkage == "extern_weak" && func {
                        continue;
                    }
                    for dso_local in [false, true] {
                        let tls_models: &[&str] = if func || linkage == "common" {
                            &[""]
                        } else {
                            &[
                                "",
                                "thread_local",
                                "thread_local(localdynamic)",
                                "thread_local(initialexec)",
                                "thread_local(localexec)",
                            ]
                        };
                        for &tls in tls_models {
                            let name = format!("s{}", names.len());
                            let dso = if dso_local { "dso_local" } else { "" };
                            let line = match (func, defined) {
                                (false, false) => {
                                    let linkage =
                                        if linkage.is_empty() { "external" } else { linkage };
                                    format!(
                                        "@{name} = {linkage} {dso} {visibility} {tls} global i32\n"
                                    )
                                }
                                (false, true) => {
                                    let align = if linkage == "common" { ", align 4" } else { "" };
                                    format!(
                                        "@{name} = {linkage} {dso} {visibility} {tls} global i32 0{align}\n"
                                    )
                                }
                                (true, false) => {
                                    format!("declare {linkage} {dso} {visibility} void @{name}()\n")
                                }
                                (true, true) => format!(
                                    "define {linkage} {dso} {visibility} void @{name}() {{\n  ret void\n}}\n"
                                ),
                            };
                            text.push_str(&line);
                            let _ = writeln!(
                                text,
                                "define ptr @get_{name}() {{\n  ret ptr @{name}\n}}"
                            );
                            if func {
                                let _ = writeln!(
                                    text,
                                    "define void @call_{name}() {{\n  call void @{name}()\n  ret void\n}}"
                                );
                            }
                            names.push(name);
                        }
                    }
                }
            }
        }
    }
    (text, names)
}

fn header(arch: Arch, major: u32, pic: bool, pie: bool) -> String {
    let (layout, triple) = match arch {
        Arch::A64 if major >= 20 => (AARCH64_LINUX_DATA_LAYOUT, "aarch64-unknown-linux-gnu"),
        Arch::A64 => (AARCH64_LINUX_DATA_LAYOUT_LLVM19, "aarch64-unknown-linux-gnu"),
        Arch::X64 => (X86_64_LINUX_DATA_LAYOUT, "x86_64-unknown-linux-gnu"),
    };
    let mut text = format!("target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n");
    match (pic, pie) {
        (true, true) => text.push_str(
            "!llvm.module.flags = !{!0, !1}\n!0 = !{i32 8, !\"PIC Level\", i32 2}\n!1 = !{i32 7, !\"PIE Level\", i32 2}\n",
        ),
        (true, false) => {
            text.push_str("!llvm.module.flags = !{!0}\n!0 = !{i32 8, !\"PIC Level\", i32 2}\n");
        }
        _ => {}
    }
    text
}

fn llc_path(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    installed.is_file().then_some(installed)
}

/// The access a getter's first relocation shows.
fn observed(arch: Arch, kind: u32) -> Option<Access> {
    Some(match (arch, kind) {
        // ADR_PREL_PG_HI21; R_X86_64_PC32, _32, _32S.
        (Arch::A64, 275) | (Arch::X64, 2 | 10 | 11) => Access::Direct,
        // ADR_GOT_PAGE; R_X86_64_GOTPCREL, _GOTPCRELX, _REX_GOTPCRELX.
        (Arch::A64, 311) | (Arch::X64, 9 | 41 | 42) => Access::Got,
        // TLSDESC_ADR_PAGE21; R_X86_64_TLSGD.
        (Arch::A64, 562) | (Arch::X64, 19) => Access::Tls(TlsAccess::GeneralDynamic),
        // R_X86_64_TLSLD.
        (Arch::X64, 20) => Access::Tls(TlsAccess::LocalDynamic),
        // TLSIE_ADR_GOTTPREL_PAGE21; R_X86_64_GOTTPOFF.
        (Arch::A64, 541) | (Arch::X64, 22) => Access::Tls(TlsAccess::InitialExec),
        // TLSLE_ADD_TPREL_HI12; R_X86_64_TPOFF32.
        (Arch::A64, 549) | (Arch::X64, 23) => Access::Tls(TlsAccess::LocalExec),
        _ => return None,
    })
}

#[test]
fn sym_access_agrees_with_llc() {
    let majors: Vec<u32> = LLVM_MAJORS.into_iter().filter(|&m| llc_path(m).is_some()).collect();
    if majors.is_empty() {
        assert!(!tools_required(), "no llc (HOD_REQUIRE_TOOLS=1)");
        return;
    }
    let scratch = Scratch::new("sym-access");
    let (body, names) = symbols();
    let mut failures = Vec::new();
    let mut checked = 0;
    for &major in &majors {
        for arch in [Arch::A64, Arch::X64] {
            for (reloc, pie) in [("static", false), ("pic", false), ("pic", true)] {
                let pic = reloc == "pic";
                let text = format!("{}{body}", header(arch, major, pic, pie));
                let config = format!("{arch:?}-{reloc}{}-{major}", if pie { "-pie" } else { "" });
                let input = scratch.write(&format!("{config}.ll"), &text);
                let output = scratch.join(format!("{config}.o"));
                let mut command = Command::new(llc_path(major).unwrap());
                command
                    .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
                    .arg("-function-sections")
                    .arg(format!("-relocation-model={reloc}"))
                    .arg(&input)
                    .arg("-o")
                    .arg(&output);
                let outcome = run_with(&mut command, &[], Duration::from_mins(2));
                assert!(outcome.success(), "{config}: llc failed:\n{}", outcome.stderr_text());
                let theirs = Elf::parse(std::fs::read(&output).unwrap());
                let parsed =
                    hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
                        .unwrap_or_else(|e| panic!("{config}: hod_ll: {e}"));
                let module: &Module = &parsed.module;
                let options = Options {
                    reloc_model: if pic { RelocModel::Pic } else { RelocModel::Static },
                    ..Options::default()
                };
                for name in &names {
                    checked += 1;
                    check(
                        &Case {
                            arch,
                            config: &config,
                            text: &text,
                            module,
                            options,
                            theirs: &theirs,
                        },
                        name,
                        &mut failures,
                    );
                }
            }
        }
    }
    eprintln!("sym_access: {checked} answers checked, {} differ", failures.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// One configuration's module and `llc`'s object for it.
struct Case<'a> {
    arch: Arch,
    config: &'a str,
    text: &'a str,
    module: &'a Module,
    options: Options,
    theirs: &'a Elf,
}

/// Checks the answers for symbol `name` against `llc`'s relocations.
fn check(case: &Case<'_>, name: &str, failures: &mut Vec<String>) {
    let Case { arch, config, text, module, options, theirs } = *case;
    let sym: SymId = module.find_symbol(name.as_bytes()).unwrap();
    let symbol = module.symbol(sym);
    let ours = sym_access(module, &options, sym);
    let relocs = theirs.relocs_of(&format!(".text.get_{name}"));
    let first = relocs.first().map(|r| r.0);
    let theirs_access = first.and_then(|kind| observed(arch, kind));
    // AArch64 lowers local-dynamic as general-dynamic.
    let expected = match (arch, ours) {
        (Arch::A64, Access::Tls(TlsAccess::LocalDynamic)) => Access::Tls(TlsAccess::GeneralDynamic),
        _ => ours,
    };
    let what = || {
        format!(
            "{config} {name} ({}): {}",
            String::from_utf8_lossy(&text_of(text, name)),
            relocs.iter().map(|r| format!("{} {}", r.0, r.1)).collect::<Vec<_>>().join(", ")
        )
    };
    if theirs_access != Some(expected) {
        failures.push(format!("{}: hod {ours:?}, llc {theirs_access:?}", what()));
    }
    // A direct reference names the section when the symbol
    // is local or reached through its local alias.
    if ours == Access::Direct && symbol.is_definition() {
        let via_section = relocs[0].1.starts_with('[');
        let local = symbol.linkage() == hod_ir::Linkage::Internal
            || symbol.linkage() == hod_ir::Linkage::Private;
        let expect = local || prefers_local_alias(module, options, sym);
        if via_section != expect {
            failures.push(format!("{}: section expected {expect}", what()));
        }
    }
    if symbol.is_function() {
        let calls = theirs.relocs_of(&format!(".text.call_{name}"));
        let call = &calls[0];
        let ours = call_access(module, sym);
        // CALL26 either way; R_X86_64_PLT32 either way.
        let kind_ok = match arch {
            Arch::A64 => call.0 == 283,
            Arch::X64 => call.0 == 4,
        };
        let via_section = call.1.starts_with('[');
        let local =
            matches!(symbol.linkage(), hod_ir::Linkage::Internal | hod_ir::Linkage::Private)
                && symbol.is_definition();
        let expect_section = local
            || (ours == hod_cg::CallAccess::Direct && prefers_local_alias(module, options, sym));
        if !kind_ok || via_section != expect_section {
            failures.push(format!(
                "{}: call {ours:?} via section {expect_section}, llc {} {}",
                what(),
                call.0,
                call.1
            ));
        }
    }
}

/// The line that declares or defines `name`.
fn text_of(text: &str, name: &str) -> Vec<u8> {
    let at = format!("@{name} ");
    let at2 = format!("@{name}()");
    text.lines()
        .find(|l| {
            (l.starts_with(&at)
                || (l.contains(&at2) && (l.starts_with("declare") || l.starts_with("define "))))
                && !l.contains("@get_")
                && !l.contains("@call_")
        })
        .unwrap_or("")
        .trim()
        .as_bytes()
        .to_vec()
}
