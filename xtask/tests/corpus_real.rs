//! `cargo xtask corpus` end to end with the real tools (`spec/harness.md`
//! "corpus"): a few tiny C programs, laid out as the GCC torture suite is,
//! are captured with clang and the real `llc`, verified through `irdiff
//! --hybrid` with the workspace's hod, and counted.
//!
//! Tools-gated like every oracle test (`docs/VALIDATION.md` "Oracles"):
//! without the tools the test skips with a note; with `HOD_REQUIRE_TOOLS=1`
//! a missing tool is a failure. It needs neither kiln nor the GCC suite.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};

use common::Scratch;
use xtask::corpus::capture::{self, Options, TORTURE};
use xtask::corpus::census::{self, D, T};
use xtask::corpus::manifest::{Manifest, Skips};
use xtask::corpus::{torture, verify};
use xtask::irdiff::tools::{self, LLVM_MAJORS, REQUIRED_LINKERS};
use xtask::irdiff::{self, Config, Mode, Search};

/// The LLVM majors that are installed with their clang, if the machine has
/// the linker driver and a linker too.
fn installed() -> Option<Vec<u32>> {
    let search = Search::from_env();
    let majors: Vec<u32> = LLVM_MAJORS
        .into_iter()
        .filter(|major| matches!(tools::find_llvm(Some(&major.to_string()), &search), Ok(Some(_))))
        .filter(|major| search.versioned("clang", *major).is_some())
        .collect();
    let driver = tools::find_driver(&search).is_some();
    let linkers: Vec<&str> = REQUIRED_LINKERS
        .into_iter()
        .filter(|name| tools::find_linker(name, &search).is_some())
        .collect();
    let found = format!("LLVM with clang {majors:?}, gcc: {driver}, linkers {linkers:?}");
    if hod_testutil::tools_required() {
        assert!(
            majors == LLVM_MAJORS && driver && linkers == REQUIRED_LINKERS,
            "required tools are missing (HOD_REQUIRE_TOOLS=1); found {found}"
        );
    }
    if majors.is_empty() || !driver || linkers.is_empty() {
        eprintln!("note: the corpus's tools not found ({found}); skipping this tool-backed test");
        return None;
    }
    Some(majors)
}

fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

const HELLO: &str = r#"#include <stdio.h>
int main(void) {
  printf("hello, %s\n", "corpus");
  return 0;
}
"#;

/// Needs the mathematics library, passes a struct by value, switches.
const SHAPES: &str = r#"#include <math.h>
#include <stdio.h>
struct pair { long a, b; };
static long sum(struct pair p) { return p.a + p.b; }
volatile double two = 2.0;
int main(void) {
  struct pair p = { 40, 2 };
  switch (sum(p)) {
    case 42: puts("forty-two"); break;
    case 1: puts("one"); break;
    default: puts("other"); break;
  }
  return sqrt(two * two) == 2.0 ? 0 : 1;
}
"#;

const ABORTS: &str = "#include <stdlib.h>\nint main(void) { abort(); }\n";

/// GNU C that only GCC compiles: a nested function.
const NESTED: &str = "int main(void) { int f(void) { return 0; } return f(); }\n";

/// Output without a final newline (`stdout-no-newline`).
const UNENDED: &str = "#include <stdio.h>\nint main(void) { printf(\"no newline\"); return 0; }\n";

/// A program the C library warns about at link time, which `ld.bfd`
/// reports and `--fatal-warnings` makes an error (`link: allow-warnings`).
const WARNS: &str = r"#include <stdio.h>
static char buffer[64];
int main(void) {
  return tmpnam(buffer) == buffer ? 0 : 1;
}
";

/// A `builtins/` test: the test, its `-lib.c` and the directory's
/// `lib/main.c`, three modules linked in that order.
const BUILTIN: &str = r#"#include <stdio.h>
#include <stdlib.h>
extern int twice(int);
void main_test(void) {
  if (twice(21) != 42) abort();
  puts("twice");
}
"#;
const BUILTIN_LIB: &str = "int twice(int x) { return 2 * x; }\n";
const BUILTIN_MAIN: &str =
    "extern void main_test(void);\nint main(void) { main_test(); return 0; }\n";

// spec/harness.md "corpus": "The gcc-torture source", "Entries",
// "Admission", "verify", "census", with the real clang, llc and linkers.
#[test]
#[allow(clippy::too_many_lines)]
fn capture_verify_and_count_real_programs() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("corpus-real");
    let execute = scratch.join("suite").join(torture::EXECUTE_DIR);
    std::fs::create_dir_all(&execute).unwrap();
    std::fs::create_dir_all(execute.join("builtins/lib")).unwrap();
    for (name, text) in [
        ("hello.c", HELLO),
        ("shapes.c", SHAPES),
        ("aborts.c", ABORTS),
        ("nested.c", NESTED),
        ("unended.c", UNENDED),
        ("warns.c", WARNS),
        ("builtins/twice.c", BUILTIN),
        ("builtins/twice-lib.c", BUILTIN_LIB),
        ("builtins/lib/main.c", BUILTIN_MAIN),
    ] {
        std::fs::write(execute.join(name), text).unwrap();
    }

    let out = scratch.join("corpus");
    let mut options = Options::new(out.clone());
    options.sources = vec![TORTURE];
    options.llvm.clone_from(&majors);
    options.torture = scratch.join("suite");
    options.require_tools = hod_testutil::tools_required();
    let report = capture::capture(&options).unwrap();
    let summary = report.summary(&out);
    assert_eq!(report.exit_status(), 0, "{summary}");
    assert_eq!(report.entries, 5 * majors.len(), "{summary}");

    let manifest = Manifest::read(&out).unwrap();
    let skips = Skips::read(&out).unwrap();
    for major in &majors {
        for test in ["hello.c", "shapes.c"] {
            let row =
                manifest.of_test(TORTURE, *major, test).unwrap_or_else(|| panic!("{summary}"));
            assert!(row.producer.contains(&format!("clang version {major}.")), "{}", row.producer);
            assert_eq!(row.llc_args, "-O0 -relocation-model=pic -filetype=obj");
            assert_eq!(row.producer_args, format!("-O0 -S -emit-llvm -w {test}"));
        }
        let entry =
            std::fs::read_to_string(out.join(format!("gcc-torture/llvm{major}/hello.c.ll")))
                .unwrap();
        assert!(entry.starts_with(";@ covers: gcc-torture hello.c\n"), "{entry}");
        assert!(entry.contains(&format!(";@ llvm: {major}\n")), "{entry}");
        assert!(entry.contains(";@ link: m\n"), "{entry}");
        assert!(entry.contains(";@ stdout: hello, corpus\n"), "{entry}");
        assert!(entry.contains("\ndefine dso_local i32 @main("), "{entry}");
        assert!(entry.contains("\n; delivered: text\n"), "{entry}");
        let entry_of = |test: &str| {
            std::fs::read_to_string(out.join(format!("gcc-torture/llvm{major}/{test}.ll"))).unwrap()
        };
        // "Test directives": an output without a final newline.
        let entry = entry_of("unended.c");
        assert!(entry.contains("\n;@ stdout-no-newline: no newline\n"), "{entry}");
        // "Admission": only ld.bfd refuses the link that warns, so the
        // entry is admitted on the second pass, which allows warnings.
        let entry = entry_of("warns.c");
        assert!(entry.contains("\n;@ link: m allow-warnings\n"), "{entry}");
        assert!(!entry_of("hello.c").contains("allow-warnings"));
        // "Several translation units": the three files, in link order.
        let entry = entry_of("builtins/twice.c");
        assert!(entry.contains("\n;@ stdout: twice\n"), "{entry}");
        assert!(entry.contains("\n; delivered: text text text\n"), "{entry}");
        let names: Vec<&str> =
            entry.lines().filter_map(|line| line.strip_prefix(";@ module: ")).collect();
        assert_eq!(names, ["test", "lib", "main"], "{entry}");
        let row = manifest.of_test(TORTURE, *major, "builtins/twice.c").unwrap();
        assert_eq!(row.delivered, "text text text");
        assert_eq!(row.producer_args.matches(" && ").count(), 2, "{}", row.producer_args);
        let reason = |test: &str| {
            skips
                .rows
                .values()
                .find(|skip| skip.llvm == *major && skip.test == test)
                .map(|skip| skip.reason.as_str())
        };
        assert_eq!(reason("aborts.c"), Some("program-fails"), "{summary}");
        assert_eq!(reason("nested.c"), Some("clang-rejects"), "{summary}");
    }

    // A second capture makes nothing and changes nothing.
    let tables = |name: &str| std::fs::read(out.join(name)).unwrap();
    let before = (tables("MANIFEST.tsv"), tables("SKIPPED.tsv"));
    let again = capture::capture(&options).unwrap();
    assert!(again.groups.iter().all(|group| group.captured == 0 && group.reused == 5));
    assert_eq!(before, (tables("MANIFEST.tsv"), tables("SKIPPED.tsv")));

    // Every entry passes `irdiff --hybrid` with the workspace's hod and
    // every linker.
    let mut config = Config::new(scratch.join("build"));
    config.mode = Mode::Hybrid;
    config.hod = Some(irdiff::build_workspace_hod(&build_dir()).expect("hod builds"));
    config.require_tools = hod_testutil::tools_required();
    let verified =
        verify::verify(&verify::Options { dir: out.clone(), config, manifest_only: false })
            .unwrap();
    assert_eq!(verified.problems, [""; 0]);
    assert_eq!(verified.exit_status(), 0, "{}", verified.summary(true));
    assert_eq!(verified.runs.len(), majors.len());
    for (_, run) in &verified.runs {
        assert_eq!(run.tests(), 5, "{}", run.summary(true));
    }

    // The census of real modules, of the two entries the counts are for.
    let entries: Vec<(String, String)> = manifest
        .rows
        .iter()
        .filter(|(_, row)| row.test == "hello.c" || row.test == "shapes.c")
        .map(|(entry, row)| (format!("{}/llvm{}", row.source, row.llvm), entry.clone()))
        .collect();
    let counts = census::count_corpus(&out, &entries, 2).unwrap();
    assert_eq!(counts.len(), majors.len());
    for (group, count) in &counts {
        assert_eq!(count.modules, 2, "{group}");
        assert_eq!(count.count(T::Opcode, "switch"), 1, "{group}");
        assert_eq!(count.count(T::Switch, "switch i64, 1-4 cases"), 1, "{group}");
        assert_eq!(count.count(T::TopLevel, "define"), 3, "{group}");
        assert_eq!(count.count(T::Opcode, "ret"), 3, "{group}");
        assert_eq!(count.count(T::CallKind, "call with an explicit function type, variadic"), 1);
        assert_eq!(count.count(T::Linkage, "define: internal"), 1, "{group}");
        assert_eq!(count.count(T::InstrFlag, "load volatile"), 2, "{group}");
        assert!(count.count(T::Predicate, "fcmp oeq") == 1, "{group}");
        assert_eq!(count.keys(T::Unrecognised), [""; 0], "{group}");
        assert_eq!(count.summary(D::Definitions).sum, 3, "{group}");
        assert!(count.count(T::ModuleFlag, "\"PIC Level\" = i32 2 (behavior 8)") == 2, "{group}");
    }
    let text = census::render(&out, 2).unwrap();
    for major in &majors {
        assert!(text.contains(&format!("| `gcc-torture/llvm{major}` | 7 | 5 | 2 |")), "{text}");
    }
}
