//! `spec/codegen.md` §7.3, "Runtime routines by type and target"
//! (WP-117): every row of `hod_cg::sym`'s routine tables against the
//! calls `llc -O0 -fast-isel=0` (LLVM 22 and 19) makes for the same
//! operation, on x86-64 and on AArch64 (`-global-isel=0`), from the IR in
//! `tests/data/runtime/`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use hod_cg::RuntimeFn;
use hod_cg::sym::{
    FpOp, FpType, fp_ext_routine, fp_routine, fp_to_int_routine, fp_trunc_routine,
    int_to_fp_routine,
};
use hod_ir::Arch;
use hod_testutil::{DEFAULT_TIMEOUT, run_with, tools_required};

const TYPES: [(FpType, &str); 5] = [
    (FpType::Half, "half"),
    (FpType::Float, "float"),
    (FpType::Double, "double"),
    (FpType::Fp128, "fp128"),
    (FpType::X86Fp80, "x86_fp80"),
];

/// Calls that widen or narrow a `half` around the operation proper: an
/// operation the table says is no call may make these.
const HALF_WIDENING: [&str; 3] = ["__extendhfsf2", "__truncsfhf2", "__truncdfhf2"];

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/runtime")
}

fn llc(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if installed.is_file() {
        return Some(installed);
    }
    assert!(!tools_required(), "no llc {major} (HOD_REQUIRE_TOOLS=1)");
    eprintln!("note: no llc {major}; skipping its part");
    None
}

/// The functions of `file` compiled by `llc` for `arch`, each with the
/// symbols it calls or tail-calls.
fn calls(llc: &Path, arch: Arch, file: &str) -> BTreeMap<String, BTreeSet<String>> {
    let (triple, extra) = match arch {
        Arch::X86_64 => ("x86_64-unknown-linux-gnu", None),
        Arch::Aarch64 => ("aarch64-unknown-linux-gnu", Some("-global-isel=0")),
    };
    let mut command = Command::new(llc);
    command.args(["-O0", "-fast-isel=0", "-relocation-model=pic", "-o", "-"]);
    command.arg(format!("-mtriple={triple}")).args(extra).arg(data().join(file));
    let out = run_with(&mut command, &[], DEFAULT_TIMEOUT);
    assert!(out.success(), "{} {file}: {}", llc.display(), out.stderr_text());
    let mut functions = BTreeMap::new();
    let mut current = None;
    for line in out.stdout_text().lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some(label) = line.strip_suffix(':')
            && !label.starts_with('.')
            && !label.contains(' ')
        {
            current = Some(label.to_owned());
            functions.entry(label.to_owned()).or_insert_with(BTreeSet::new);
            continue;
        }
        let mut words = line.split_whitespace();
        let (Some(op), Some(target), Some(name)) = (words.next(), words.next(), current.as_ref())
        else {
            continue;
        };
        if matches!(op, "call" | "callq" | "jmp" | "bl" | "b") && !target.starts_with('.') {
            let target = target.trim_end_matches("@PLT").to_owned();
            functions.get_mut(name).expect("the current function").insert(target);
        }
    }
    functions
}

/// What one function is expected to call: `Some(name)` among its calls,
/// or `None`: no call but `half` widening.
fn check(
    what: &str,
    expected: Option<RuntimeFn>,
    calls: &BTreeSet<String>,
    errors: &mut Vec<String>,
) {
    let ok = match expected {
        Some(func) => calls.contains(func.name()),
        None => calls.iter().all(|c| HALF_WIDENING.contains(&c.as_str())),
    };
    if !ok {
        errors.push(format!(
            "{what}: table {:?}, llc calls {calls:?}",
            expected.map(RuntimeFn::name)
        ));
    }
}

fn op_name(op: FpOp) -> String {
    format!("op_{op:?}").to_lowercase()
}

fn width_name(bits: u32) -> String {
    format!("i{bits}")
}

/// Rows where `llc` 19 differs from 22 or does not know the intrinsic;
/// the tables follow LLVM 22 (kiln's).
fn llc19_differs(arch: Arch, ty: FpType, op: FpOp) -> bool {
    matches!(op, FpOp::Atan2 | FpOp::MinimumNum | FpOp::MaximumNum)
        || (arch == Arch::X86_64 && ty == FpType::Half && matches!(op, FpOp::Fma | FpOp::Powi))
}

/// Rows where hod deliberately differs from `llc` (`fp_ext_routine`'s
/// documentation): AArch64 widens `half` to `float` before
/// `__extendsftf2`.
fn hod_differs(arch: Arch, from: FpType, to: FpType) -> bool {
    arch == Arch::Aarch64 && from == FpType::Half && to == FpType::Fp128
}

fn compare(major: u32, arch: Arch, errors: &mut Vec<String>) {
    let Some(llc) = llc(major) else { return };
    for (ty, ty_name) in TYPES {
        if arch == Arch::Aarch64 && ty == FpType::X86Fp80 {
            continue;
        }
        let ops = calls(&llc, arch, &format!("ops.{ty_name}.ll"));
        for op in FpOp::ALL {
            let Some(found) = ops.get(&op_name(op)) else { continue };
            if major == 19 && llc19_differs(arch, ty, op) {
                continue;
            }
            let what = format!("llc {major} {arch:?} {ty_name} {op:?}");
            check(&what, fp_routine(op, ty, arch), found, errors);
        }
        let casts = calls(&llc, arch, &format!("casts.{ty_name}.ll"));
        for (to, to_name) in TYPES {
            if hod_differs(arch, ty, to) {
                continue;
            }
            let (ext, trunc) = (format!("ext_{to_name}"), format!("trunc_{to_name}"));
            if let Some(found) = casts.get(&ext) {
                let what = format!("llc {major} {arch:?} fpext {ty_name} to {to_name}");
                check(&what, fp_ext_routine(ty, to, arch), found, errors);
            }
            if let Some(found) = casts.get(&trunc) {
                let what = format!("llc {major} {arch:?} fptrunc {ty_name} to {to_name}");
                check(&what, fp_trunc_routine(ty, to, arch), found, errors);
            }
            if arch == Arch::X86_64 && ty == FpType::X86Fp80 && to != FpType::X86Fp80 {
                let Some(found) = casts.get(&format!("from_{to_name}")) else { continue };
                let what = format!("llc {major} {arch:?} {to_name} to x86_fp80");
                let expected = fp_ext_routine(to, ty, arch).or(fp_trunc_routine(to, ty, arch));
                check(&what, expected, found, errors);
            }
        }
        for bits in [32, 64, 128] {
            for signed in [true, false] {
                let (to_int, to_fp) =
                    if signed { ("fptosi", "sitofp") } else { ("fptoui", "uitofp") };
                let width = width_name(bits);
                if let Some(found) = casts.get(&format!("{to_int}_{width}")) {
                    let what = format!("llc {major} {arch:?} {to_int} {ty_name} to {width}");
                    check(&what, fp_to_int_routine(ty, signed, bits, arch), found, errors);
                }
                if let Some(found) = casts.get(&format!("{to_fp}_{width}")) {
                    let what = format!("llc {major} {arch:?} {to_fp} {width} to {ty_name}");
                    check(&what, int_to_fp_routine(signed, bits, ty, arch), found, errors);
                }
            }
        }
    }
}

/// §7.3: every routine of the tables is the one `llc` calls, and every
/// operation the tables call inline makes no call (but `half` widening).
#[test]
fn tables_match_llc() {
    let mut errors = Vec::new();
    for major in [22, 19] {
        for arch in [Arch::X86_64, Arch::Aarch64] {
            compare(major, arch, &mut errors);
        }
    }
    assert!(errors.is_empty(), "{} rows differ:\n{}", errors.len(), errors.join("\n"));
}
