//! Module-level rules against `llc` (`spec/ir.md` §2.2): the two places
//! where the symbol table stores something other than what the producer
//! wrote, each checked against what `llc` does with the same input.
//!
//! - A constructor priority above 65535 is stored as 65535: `llc` puts
//!   such an entry in the plain `.init_array` section.
//! - `dso_local` is implied by a local linkage and by a non-default
//!   visibility (except on `extern_weak`): under `-relocation-model=pic`
//!   `llc` takes the address of exactly the other symbols through the
//!   GOT (plus, on AArch64, every `extern_weak` symbol).

mod common;

use std::fmt::Write as _;

use common::{CONFIGS, Config, tools};
use hod_ir::{Arch, FunctionDesc, Linkage, Module, TargetSpec, TypeId, VariableDesc, Visibility};
use hod_testutil::Scratch;

fn target(config: &Config) -> TargetSpec {
    match config.arch {
        Arch::Aarch64 => TargetSpec::aarch64_linux_gnu(),
        Arch::X86_64 => TargetSpec::x86_64_linux_gnu(),
    }
}

/// The section each `.quad <name>` line of `asm` is in.
fn quads_by_section(asm: &str) -> Vec<(String, String)> {
    let mut section = String::new();
    let mut quads = Vec::new();
    for line in asm.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(".section") {
            section = rest.trim().split(',').next().unwrap_or("").to_string();
        } else if let Some(rest) =
            line.strip_prefix(".quad").or_else(|| line.strip_prefix(".xword"))
        {
            quads.push((rest.trim().to_string(), section.clone()));
        }
    }
    quads
}

/// §2.2: `Module::add_ctor` stores the priority `llc` acts on. The
/// section of an entry is `.init_array` for priority 65535 and
/// `.init_array.<priority>` otherwise, and `llc` treats every `i32`
/// above 65535, read as unsigned, as 65535.
#[test]
fn structor_priorities_are_clamped_as_llc_clamps_them() {
    let priorities: [i32; 11] = [0, 1, 5, 101, 65534, 65535, 65536, 70000, -1, i32::MIN, i32::MAX];
    for config in &CONFIGS {
        let Some(tools) = tools(config.llvm) else { continue };
        let scratch = Scratch::new(&format!("structors-{}", config.name()));
        let mut module = Module::new(target(config));
        let void_fn = module.types.function(TypeId::VOID, &[], false).unwrap();
        let mut ir = config.header();
        let entry = |array: &mut String, priority: i32, name: &str| {
            write!(array, "{{ i32, ptr, ptr }} {{ i32 {priority}, ptr @{name}, ptr null }}, ")
                .unwrap();
        };
        let (mut ctors, mut dtors) = (String::new(), String::new());
        for (index, priority) in priorities.into_iter().enumerate() {
            for (kind, array) in [("ctor", &mut ctors), ("dtor", &mut dtors)] {
                let name = format!("{kind}{index}");
                entry(array, priority, &name);
                writeln!(ir, "declare void @{name}()").unwrap();
                let func =
                    module.add_function(name.as_bytes(), &FunctionDesc::new(void_fn)).unwrap();
                // The producer passes the `i32` as its bits.
                #[allow(clippy::cast_sign_loss)]
                let bits = priority as u32;
                if kind == "ctor" {
                    module.add_ctor(bits, func, None);
                } else {
                    module.add_dtor(bits, func, None);
                }
            }
        }
        for (name, array) in [("ctors", &ctors), ("dtors", &dtors)] {
            let count = priorities.len();
            let body = array.trim_end_matches(", ");
            writeln!(
                ir,
                "@llvm.global_{name} = appending global [{count} x {{ i32, ptr, ptr }}] [{body}]"
            )
            .unwrap();
        }
        let asm = tools.compile(&scratch, "structors", &ir, true).unwrap_or_else(|e| panic!("{e}"));
        let quads = quads_by_section(&std::fs::read_to_string(asm).unwrap());
        assert_eq!(quads.len(), 2 * priorities.len(), "{}", config.name());
        for (list, base) in [(module.ctors(), ".init_array"), (module.dtors(), ".fini_array")] {
            assert_eq!(list.len(), priorities.len());
            for structor in list {
                let name =
                    String::from_utf8_lossy(module.symbol(structor.func).name()).into_owned();
                let expected = match structor.priority {
                    65535 => base.to_string(),
                    priority => format!("{base}.{priority}"),
                };
                let (_, section) = quads
                    .iter()
                    .find(|(quad, _)| *quad == name)
                    .unwrap_or_else(|| panic!("{}: no entry for {name}", config.name()));
                assert_eq!(*section, expected, "{}: {name}", config.name());
            }
        }
    }
}

/// §2.2: the stored `dso_local` is the one `llc` computes. Every valid
/// combination of linkage, visibility, stated `dso_local` and
/// declaration or definition is a variable whose address a function
/// returns; with PIC, `llc` goes through the GOT exactly when hod's
/// `Symbol::dso_local` is false.
#[test]
fn implied_dso_local_is_what_llc_implies() {
    for config in &CONFIGS {
        let Some(tools) = tools(config.llvm) else { continue };
        let scratch = Scratch::new(&format!("dso-local-{}", config.name()));
        let mut module = Module::new(target(config));
        let mut ir = config.header();
        let mut expected = Vec::new();
        let zero = module.consts.int(&module.types, TypeId::I32, 0).unwrap();
        for linkage in Linkage::ALL {
            for visibility in [Visibility::Default, Visibility::Hidden, Visibility::Protected] {
                for dso_local in [false, true] {
                    for defined in [false, true] {
                        let name = format!("v{}e", expected.len());
                        let desc = VariableDesc {
                            linkage,
                            visibility,
                            dso_local,
                            init: defined.then_some(zero),
                            ..VariableDesc::new(TypeId::I32)
                        };
                        // The combinations LLVM has no text for are the
                        // ones hod refuses (`symbols_as.rs`).
                        let mut probe = module.clone();
                        let valid = probe.add_variable(name.as_bytes(), &desc).is_ok()
                            && probe.verify_symbols().is_ok();
                        if !valid || linkage == Linkage::AvailableExternally {
                            continue;
                        }
                        let sym = module.add_variable(name.as_bytes(), &desc).unwrap();
                        let keyword = match (linkage, defined) {
                            (Linkage::External, true) => String::new(),
                            _ => format!("{} ", linkage.keyword()),
                        };
                        let local = if dso_local { "dso_local " } else { "" };
                        let visible = match visibility {
                            Visibility::Default => "",
                            Visibility::Hidden => "hidden ",
                            Visibility::Protected => "protected ",
                        };
                        let init = if defined { " 0" } else { "" };
                        writeln!(ir, "@{name} = {keyword}{local}{visible}global i32{init}")
                            .unwrap();
                        writeln!(ir, "define ptr @f{name}() {{\n  ret ptr @{name}\n}}").unwrap();
                        expected.push((name, module.symbol(sym).dso_local(), desc));
                    }
                }
            }
        }
        assert!(expected.len() >= 40, "{} combinations", expected.len());
        assert!(expected.iter().any(|(_, local, _)| *local));
        assert!(expected.iter().any(|(_, local, _)| !*local));
        let input = scratch.write("dso.ll", &ir);
        let output = scratch.join("dso.s");
        let outcome = hod_testutil::run(
            &tools.llc,
            [
                "-O0".as_ref(),
                "-relocation-model=pic".as_ref(),
                "-filetype=asm".as_ref(),
                "-o".as_ref(),
                output.as_os_str(),
                input.as_os_str(),
            ],
        );
        assert!(outcome.success(), "{}: {}", config.name(), outcome.stderr_text());
        let asm = std::fs::read_to_string(output).unwrap();
        for (name, local, desc) in &expected {
            let through_got = match config.arch {
                Arch::X86_64 => asm.contains(&format!("{name}@GOTPCREL")),
                Arch::Aarch64 => asm.contains(&format!(":got:{name}")),
            };
            // AArch64 reaches an `extern_weak` symbol through the GOT even
            // when it is stated to be `dso_local`: its address may be zero,
            // which `adrp` cannot produce. That is the target's choice, not
            // a different `dso_local`.
            let always_got = config.arch == Arch::Aarch64 && desc.linkage == Linkage::ExternWeak;
            assert_eq!(through_got, !local || always_got, "{}: {name} is {desc:?}", config.name());
        }
    }
}
