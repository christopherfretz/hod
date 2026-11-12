//! §7.1: the module properties `hod_ll` reads out of `!llvm.module.flags`
//! for the clang options that set them, against a table of what each
//! option means.

use hod_ir::{Arch, BranchProtection, CodeModel, FramePointer, ModuleProps, PicLevel, UwTable};

use crate::common::{ARCHES, Llvm, arch_name, parse};

/// The properties of a module compiled with no option, on Linux: a
/// position-independent executable with asynchronous unwind tables, and
/// a frame pointer in every function on x86-64 and in non-leaf functions
/// on AArch64 (where clang 22 also stopped reserving the register in the
/// others).
fn defaults(arch: Arch, major: u32) -> ModuleProps {
    ModuleProps {
        pic: PicLevel::Big,
        pie: PicLevel::Big,
        uwtable: UwTable::Async,
        frame_pointer: match arch {
            Arch::Aarch64 if major < 22 => FramePointer::NonLeaf,
            Arch::Aarch64 => FramePointer::NonLeafNoReserve,
            Arch::X86_64 => FramePointer::All,
        },
        ..ModuleProps::default()
    }
}

type Change = fn(&mut ModuleProps);

/// Each row: the target it applies to (`None` for both), the clang
/// options, and what they change in [`defaults`].
const TABLE: &[(Option<Arch>, &[&str], Change)] = &[
    (None, &[], |_| {}),
    (None, &["-fPIC"], |props| props.pie = PicLevel::None),
    (None, &["-fpic"], |props| (props.pic, props.pie) = (PicLevel::Small, PicLevel::None)),
    (None, &["-fPIE"], |_| {}),
    (None, &["-fpie"], |props| (props.pic, props.pie) = (PicLevel::Small, PicLevel::Small)),
    (None, &["-fno-pic"], |props| (props.pic, props.pie) = (PicLevel::None, PicLevel::None)),
    (None, &["-funwind-tables", "-fno-asynchronous-unwind-tables"], |props| {
        props.uwtable = UwTable::Sync;
    }),
    (None, &["-fno-asynchronous-unwind-tables"], |props| props.uwtable = UwTable::None),
    (None, &["-fno-omit-frame-pointer", "-mno-omit-leaf-frame-pointer"], |props| {
        props.frame_pointer = FramePointer::All;
    }),
    (None, &["-fomit-frame-pointer"], |props| props.frame_pointer = FramePointer::None),
    (None, &["-g"], |props| {
        (props.dwarf_version, props.debug_info_version) = (Some(5), Some(3));
    }),
    (None, &["-gdwarf-4"], |props| {
        (props.dwarf_version, props.debug_info_version) = (Some(4), Some(3));
    }),
    (Some(Arch::Aarch64), &["-mbranch-protection=standard"], |props| {
        // `standard` is BTI, return address signing and GCS.
        props.branch_protection = BranchProtection::from_bits(
            BranchProtection::BTI | BranchProtection::PAC_RET | BranchProtection::GCS,
        );
    }),
    (Some(Arch::Aarch64), &["-mbranch-protection=pac-ret+leaf+b-key"], |props| {
        props.branch_protection = BranchProtection::from_bits(
            BranchProtection::PAC_RET | BranchProtection::PAC_RET_ALL | BranchProtection::PAC_B_KEY,
        );
    }),
    (Some(Arch::Aarch64), &["-mbranch-protection=bti"], |props| {
        props.branch_protection = BranchProtection::from_bits(BranchProtection::BTI);
    }),
    (Some(Arch::Aarch64), &["-mcmodel=tiny", "-fno-pic"], |props| {
        (props.pic, props.pie) = (PicLevel::None, PicLevel::None);
        props.code_model = Some(CodeModel::Tiny);
    }),
    (Some(Arch::X86_64), &["-fcf-protection=full"], |props| {
        props.branch_protection =
            BranchProtection::from_bits(BranchProtection::CET_IBT | BranchProtection::CET_SHSTK);
    }),
    (Some(Arch::X86_64), &["-fcf-protection=branch"], |props| {
        props.branch_protection = BranchProtection::from_bits(BranchProtection::CET_IBT);
    }),
    (Some(Arch::X86_64), &["-mcmodel=large"], |props| {
        // Everything is large data in the large model.
        (props.code_model, props.large_data_threshold) = (Some(CodeModel::Large), Some(0));
    }),
    (Some(Arch::X86_64), &["-mcmodel=medium"], |props| {
        props.code_model = Some(CodeModel::Medium);
        props.large_data_threshold = Some(65536);
    }),
    (Some(Arch::X86_64), &["-mcmodel=medium", "-mlarge-data-threshold=1000"], |props| {
        props.code_model = Some(CodeModel::Medium);
        props.large_data_threshold = Some(1000);
    }),
    (Some(Arch::X86_64), &["-mcmodel=kernel", "-fno-pic"], |props| {
        (props.pic, props.pie) = (PicLevel::None, PicLevel::None);
        props.code_model = Some(CodeModel::Kernel);
    }),
];

#[test]
fn module_properties_match_the_clang_options() {
    let mut failures = Vec::new();
    for llvm in Llvm::all() {
        for arch in ARCHES {
            for &(only, options, change) in TABLE {
                if only.is_some_and(|only| only != arch) {
                    continue;
                }
                let text = llvm.clang("min.c", arch, options);
                let parsed = parse(&text, arch).expect("clang's output parses");
                let mut expected = defaults(arch, llvm.major);
                change(&mut expected);
                if parsed.module.props != expected {
                    failures.push(format!(
                        "clang-{} {} {options:?}:\n  read     {:?}\n  expected {expected:?}",
                        llvm.major,
                        arch_name(arch),
                        parsed.module.props
                    ));
                }
                // Nothing clang writes for these options is left over
                // as a flag that has to be honoured and is not known.
                for flag in parsed.module.other_flags() {
                    assert!(flag.ignorable, "{options:?}: flag {flag:?}");
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
