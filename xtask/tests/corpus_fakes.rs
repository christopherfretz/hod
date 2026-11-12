//! `cargo xtask corpus` (`spec/harness.md` "corpus") with fake tools: a
//! `kiln++`, a `clang`, an `llc`, a linker driver and linkers that are
//! shell scripts, so capture, admission, the tables and verification can
//! be exercised on any Linux machine, without kiln, LLVM or the GCC suite.
//!
//! How the fakes work (the linkers and the objects are those of
//! `irdiff_fakes.rs`). A test's source holds its "module" in lines that
//! start with `//; `: the fake producers write them out as `; ` lines. A
//! module's `; program: <command>` lines become the program; `;
//! program(mold): ...` only when mold links it. Other marker lines make a
//! tool misbehave: `//; kiln: fail|twice|bitcode|nollc|static|tune`,
//! `//; clang: reject`, `; llc: fail`, `; link-fail: llc bfd`.
#![cfg(target_os = "linux")]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::Scratch;
use hod_testutil::write_script;
use xtask::corpus::capture::{self, CaptureReport, KILN, Options, TORTURE};
use xtask::corpus::manifest::{Manifest, Skips};
use xtask::corpus::{census, verify};
use xtask::irdiff::{Arch, Config, Mode, Search};

const FAKE_LLC: &str = r#"PATH=/usr/bin:/bin
case "$0" in */llvm-19/*) major=19 ;; *) major=22 ;; esac
if [ "$1" = --version ]; then echo "fake LLVM version $major.0.0"; exit 0; fi
out= in= args=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -) in=- ;;
    -*) args="$args $1" ;;
    *) in=$1 ;;
  esac
  shift
done
if [ "$in" = - ]; then in="$out.in"; cat > "$in"; fi
if grep -q '^; llc: fail' "$in"; then echo "llc: error: bad module" >&2; exit 1; fi
{ echo "built-by llc"; echo "args$args"; cat "$in"; } > "$out""#;

/// Declines every module: in hybrid mode the fallback builds it.
const FAKE_HOD: &str = r#"PATH=/usr/bin:/bin
log= fallback= out= in= args=
while [ $# -gt 0 ]; do
  case "$1" in
    --coverage-log=*) log=${1#--coverage-log=} ;;
    --strict) ;;
    --fallback=*) fallback=${1#--fallback=} ;;
    -o) out=$2; shift ;;
    -*) args="$args $1" ;;
    *) in=$1 ;;
  esac
  shift
done
if [ -n "$fallback" ]; then
  "$fallback" $args -o "$out" "$in" || exit 1
  printf 'fallback\tcodegen-not-implemented\ttext\t1\t%s\t-\t5\t-\n' "$out" >> "$log"
  exit 0
fi
printf 'error\tcodegen-not-implemented\ttext\t1\t%s\t-\t5\t-\n' "$out" >> "$log"
exit 2"#;

/// With `-B<dir>` the linker is `<dir>/ld`, as for the real driver;
/// without, GNU ld's stand-in.
const FAKE_DRIVER: &str = r#"dir=
for arg in "$@"; do case "$arg" in -B*) dir=${arg#-B} ;; esac; done
if [ -n "$dir" ]; then exec "$dir/ld" "$@"; fi
exec "$(dirname "$0")/ld.bfd" "$@""#;

/// The programs carry the signature the real linker leaves: `irdiff`
/// checks that the driver used the linker it was told to.
fn fake_linker(name: &str) -> String {
    let signature = match name {
        "lld" => "echo '# Linker: LLD 22.0.0'",
        "mold" => "echo '# mold 2.0.0 (compatible with GNU ld)'",
        _ => ":",
    };
    format!(
        r#"PATH=/usr/bin:/bin
me={name}
out= obj=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -*) ;;
    *) obj=$1 ;;
  esac
  shift
done
builder=$(sed -n '1s/^built-by //p' "$obj")
if grep -q "^; link-fail: $builder $me\$" "$obj"; then
  echo "$me: error: undefined symbol: main" >&2
  exit 1
fi
{{
  echo '#!/bin/sh'
  {signature}
  echo "PATH=/usr/bin:/bin"
  sed -n -e 's/^; program: //p' -e "s/^; program($me): //p" "$obj"
}} > "$out"
chmod 755 "$out""#
    )
}

/// `calls` is a file every run of the producer adds a line to.
fn fake_kiln(calls: &Path) -> String {
    format!(
        r#"PATH=/usr/bin:/bin
if [ "$1" = --version ]; then echo "fake kiln version 0.1"; exit 0; fi
dir= out= src= opts=
while [ $# -gt 0 ]; do
  case "$1" in
    --llvm-dir=*) dir=${{1#--llvm-dir=}} ;;
    -o) out=$2; shift ;;
    -x) shift ;;
    -std=*|-D*) opts="$opts $1" ;;
    -*) ;;
    *) src=$1 ;;
  esac
  shift
done
echo "kiln $src" >> '{calls}'
if [ ! -f "$src" ]; then echo "kiln++: error: no such file: $src" >&2; exit 1; fi
has() {{ grep -q "^//; kiln: $1" "$src"; }}
if has fail; then echo "$src:1:1: error: this is not C++" >&2; exit 1; fi
if has nollc; then printf '#!/bin/sh\n' > "$out"; chmod 755 "$out"; exit 0; fi
tmp="${{TMPDIR:?}}/kiln-$$"
mkdir -p "$tmp" || exit 1
model=pic; if has static; then model=static; fi
tune=; if has tune; then tune=-debugger-tune=gdb; fi
module() {{
  if has bitcode; then printf 'BC\300\336'; fi
  echo "; options:$opts"
  sed -n 's|^//; |; |p' "$src"
}}
module | "$dir/bin/llc" -O0 -relocation-model=$model -filetype=obj $tune -o "$tmp/0-t.o" - || exit 1
if has twice; then
  module | "$dir/bin/llc" -O0 -relocation-model=$model -filetype=obj -o "$tmp/1-t.o" - || exit 1
fi
"$dir/bin/clang++" "$tmp/0-t.o" -o "$out" || exit 1"#,
        calls = calls.display()
    )
}

fn fake_clang(calls: &Path) -> String {
    format!(
        r#"PATH=/usr/bin:/bin
if [ "$1" = --version ]; then echo "fake clang version 22.0.0"; exit 0; fi
out= src= flags=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -*) flags="$flags $1" ;;
    *) src=$1 ;;
  esac
  shift
done
echo "clang $src" >> '{calls}'
if grep -q '^//; clang: reject' "$src"; then echo "$src:1:1: error: GNU C" >&2; exit 1; fi
{{ echo "; flags:$flags"; sed -n 's|^//; |; |p' "$src"; }} > "$out""#,
        calls = calls.display()
    )
}

/// A machine made of fakes, with a kiln checkout and a torture suite.
struct World {
    scratch: Scratch,
}

impl World {
    fn new(label: &str) -> Self {
        let world = Self { scratch: Scratch::new(label) };
        for dir in ["bin", "build", "kiln/tests/conformance", "kiln/resource"] {
            std::fs::create_dir_all(world.scratch.join(dir)).unwrap();
        }
        std::fs::create_dir_all(world.execute()).unwrap();
        std::fs::write(world.calls(), "").unwrap();
        world.tool("gcc", FAKE_DRIVER);
        world.tool("ld.bfd", &fake_linker("bfd"));
        world.tool("ld.lld-22", &fake_linker("lld"));
        world.tool("mold", &fake_linker("mold"));
        world.tool("kiln", &fake_kiln(&world.calls()));
        // As installed: `kiln++` is a link to `kiln`.
        std::os::unix::fs::symlink("kiln", world.scratch.join("bin/kiln++")).unwrap();
        write_script(&world.scratch.join("hod"), FAKE_HOD);
        world.llvm(22);
        world
    }

    /// Installs a fake LLVM: `<root>/llvm-<major>` with the tools kiln and
    /// the capture use.
    fn llvm(&self, major: u32) {
        let dir = self.scratch.join(format!("llvm/llvm-{major}"));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("lib/clang").join(major.to_string())).unwrap();
        write_script(&dir.join("bin/llc"), FAKE_LLC);
        write_script(&dir.join("bin/opt"), "exit 1");
        let link = format!("exec '{}' \"$@\"", self.scratch.join("bin/ld.bfd").display());
        write_script(&dir.join("bin/clang++"), &link);
        write_script(&dir.join("bin/clang"), &fake_clang(&self.calls()));
    }

    fn tool(&self, name: &str, body: &str) {
        write_script(&self.scratch.join("bin").join(name), body);
    }

    fn calls(&self) -> PathBuf {
        self.scratch.join("calls")
    }

    /// How often the producers ran.
    fn call_count(&self) -> usize {
        std::fs::read_to_string(self.calls()).unwrap().lines().count()
    }

    fn execute(&self) -> PathBuf {
        self.scratch.join("gcc").join(xtask::corpus::torture::EXECUTE_DIR)
    }

    fn out(&self) -> PathBuf {
        self.scratch.join("corpus")
    }

    /// Adds a kiln test; `name` is relative to `tests/`.
    fn kiln_test(&self, name: &str, text: &str) {
        let path = self.scratch.join("kiln/tests").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Adds a torture test; `name` is relative to `execute/`.
    fn torture_test(&self, name: &str, text: &str) {
        let path = self.execute().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn search(&self) -> Search {
        Search { path: self.scratch.join("bin").into(), llvm_root: self.scratch.join("llvm") }
    }

    fn options(&self) -> Options {
        let mut options = Options::new(self.out());
        options.llvm = vec![22];
        options.kiln = self.scratch.join("bin/kiln++");
        options.kiln_repo = self.scratch.join("kiln");
        options.torture = self.scratch.join("gcc");
        options.search = self.search();
        options
    }

    fn capture(&self) -> CaptureReport {
        capture::capture(&self.options()).unwrap()
    }

    fn verify_options(&self) -> verify::Options {
        let mut config = Config::new(self.scratch.join("build"));
        config.mode = Mode::Hybrid;
        config.hod = Some(self.scratch.join("hod"));
        config.search = self.search();
        verify::Options { dir: self.out(), config, manifest_only: false }
    }

    /// The corpus's deterministic files, by path, with their contents.
    fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(dir: &Path, prefix: &str, into: &mut BTreeMap<String, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
                if entry.file_type().unwrap().is_dir() {
                    walk(&entry.path(), &format!("{name}/"), into);
                } else if name != capture::RUNS {
                    into.insert(name, std::fs::read(entry.path()).unwrap());
                }
            }
        }
        let mut files = BTreeMap::new();
        walk(&self.out(), "", &mut files);
        files
    }
}

fn host() -> Arch {
    Arch::host().expect("these tests run on Linux, on aarch64 or x86_64")
}

/// `source/llvm/test -> reason` of every skipped test.
fn reasons(world: &World) -> BTreeMap<String, String> {
    Skips::read(&world.out())
        .unwrap()
        .rows
        .values()
        .map(|skip| (format!("{}/{}/{}", skip.source, skip.llvm, skip.test), skip.reason.clone()))
        .collect()
}

fn entries(world: &World) -> Vec<String> {
    Manifest::read(&world.out()).unwrap().rows.keys().cloned().collect()
}

/// A world with one test of every kind the capture tells apart.
fn full_world(label: &str) -> World {
    let world = World::new(label);
    let run = "//@ expect: run\n";
    for (name, text) in [
        ("conformance/a/hello.cpp", "//@ stdout: hello\n//; program: echo hello\n".to_owned()),
        (
            "conformance/a/status.cpp",
            "//@ std: c++23\n//@ exit: 3\n//@ run-args: x y\n//@ run-stdin: in\n\
             //@ stdout: in\n//@ stdout: x-y\n\
             //; program: cat\n//; program: echo \"$1-$2\"\n//; program: exit 3\n"
                .to_owned(),
        ),
        ("conformance/a/static.cpp", "//; kiln: static\n//; program: exit 0\n".to_owned()),
        ("conformance/a/tune.cpp", "//; kiln: tune\n//; program: exit 0\n".to_owned()),
        ("c/run/plain.c", "//; program: echo c\n".to_owned()),
        ("conformance/b/two.cpp", "//; kiln: twice\n".to_owned()),
        ("conformance/b/opt.cpp", "//@ args: -O2\n//; kiln: bitcode\n".to_owned()),
        ("conformance/b/coro.cpp", "//; kiln: bitcode\n// co_await x;\n".to_owned()),
        ("conformance/b/inline.cpp", "//; kiln: bitcode\n".to_owned()),
        ("conformance/b/lto.cpp", "//@ args: -flto\n//; kiln: nollc\n".to_owned()),
        ("conformance/b/nollc.cpp", "//; kiln: nollc\n".to_owned()),
        ("conformance/b/bad.cpp", "//; kiln: fail\n".to_owned()),
        ("conformance/b/nolink.cpp", "//; link-fail: llc bfd\n".to_owned()),
        ("conformance/b/wrong.cpp", "//@ stdout: a\n//; program: echo b\n".to_owned()),
        ("conformance/b/status.cpp", "//; program: exit 1\n".to_owned()),
        ("conformance/b/mold.cpp", "//; program(mold): echo more\n".to_owned()),
        ("conformance/b/xfail.cpp", "//@ xfail: not yet\n".to_owned()),
        ("conformance/b/unit.cpp", "//@ unit: other.cpp\n".to_owned()),
    ] {
        world.kiln_test(name, &format!("{run}{text}"));
    }
    // Not an executing test: not seen at all.
    world.kiln_test("conformance/a/compile.cpp", "//@ expect: compile\n");
    for (name, text) in [
        ("ok-1.c", "//; program: exit 0\n"),
        ("out.c", "//; program: echo out\n"),
        ("flags.c", "/* { dg-options \"-fwrapv\" } */\n//; program: exit 0\n"),
        ("ieee/inf.c", "//; program: exit 0\n"),
        ("abort.c", "//; program: kill -ABRT $$\n"),
        ("status.c", "//; program: exit 1\n"),
        ("nested.c", "//; clang: reject\n"),
        ("llc.c", "//; llc: fail\n"),
        ("ld.c", "//; link-fail: llc bfd\n"),
        ("special.c", "/* { dg-options \"-Wl,--defsym,x=1\" } */\n"),
        ("builtins/abs.c", "//; program: exit 0\n"),
        ("mold.c", "//; program(mold): echo more\n"),
    ] {
        world.torture_test(name, text);
    }
    world
}

// spec/harness.md "corpus": "capture", "Entries", "Skip reasons",
// "Admission", "verify", "census". One capture of both sources: what is
// captured, what each skipped test is skipped for, what an entry is, and
// that the corpus then verifies and can be counted.
#[test]
#[expect(clippy::too_many_lines, reason = "one capture, looked at from every side")]
fn capture_then_verify() {
    let world = full_world("corpus-capture");
    let report = world.capture();
    assert_eq!(report.exit_status(), 0, "{}", report.summary(&world.out()));
    assert_eq!(report.missing_tools, [""; 0]);
    // `wild` is optional and not there.
    assert_eq!(report.notes.len(), 1, "{:?}", report.notes);

    assert_eq!(
        entries(&world),
        [
            "gcc-torture/llvm22/flags.c.ll",
            "gcc-torture/llvm22/ieee/inf.c.ll",
            "gcc-torture/llvm22/ok-1.c.ll",
            "gcc-torture/llvm22/out.c.ll",
            "kiln/llvm22/c/run/plain.c.ll",
            "kiln/llvm22/conformance/a/hello.cpp.ll",
            "kiln/llvm22/conformance/a/static.cpp.ll",
            "kiln/llvm22/conformance/a/status.cpp.ll",
            "kiln/llvm22/conformance/a/tune.cpp.ll",
        ]
    );
    let wanted: BTreeMap<String, String> = [
        ("gcc-torture/22/abort.c", "program-fails"),
        ("gcc-torture/22/builtins/abs.c", "multi-file"),
        ("gcc-torture/22/ld.c", "link-fails"),
        ("gcc-torture/22/llc.c", "llc-fails"),
        ("gcc-torture/22/mold.c", "not-reproducible"),
        ("gcc-torture/22/nested.c", "clang-rejects"),
        ("gcc-torture/22/special.c", "special-flags"),
        ("gcc-torture/22/status.c", "program-fails"),
        ("kiln/22/tests/conformance/b/unit.cpp", "multi-file"),
        ("kiln/22/tests/conformance/b/bad.cpp", "compile-fails"),
        ("kiln/22/tests/conformance/b/coro.cpp", "opt-first-coroutine"),
        ("kiln/22/tests/conformance/b/inline.cpp", "opt-first-always-inline"),
        ("kiln/22/tests/conformance/b/lto.cpp", "opt-first-lto"),
        ("kiln/22/tests/conformance/b/mold.cpp", "not-reproducible"),
        ("kiln/22/tests/conformance/b/nolink.cpp", "link-fails"),
        ("kiln/22/tests/conformance/b/nollc.cpp", "no-module"),
        ("kiln/22/tests/conformance/b/opt.cpp", "opt-first-optimized"),
        ("kiln/22/tests/conformance/b/status.cpp", "expectation-mismatch"),
        ("kiln/22/tests/conformance/b/two.cpp", "multi-file"),
        ("kiln/22/tests/conformance/b/wrong.cpp", "expectation-mismatch"),
        ("kiln/22/tests/conformance/b/xfail.cpp", "marked-xfail"),
    ]
    .into_iter()
    .map(|(test, reason)| (test.to_owned(), reason.to_owned()))
    .collect();
    assert_eq!(reasons(&world), wanted);

    // The report's numbers are the tables'.
    let stats: Vec<(&str, u32, usize, usize, usize)> = report
        .groups
        .iter()
        .map(|g| (g.source.as_str(), g.llvm, g.tests, g.captured, g.skipped.values().sum()))
        .collect();
    assert_eq!(stats, [(KILN, 22, 18, 5, 13), (TORTURE, 22, 12, 4, 8)]);
    assert_eq!(report.entries, 9);

    // An entry: the directives, the provenance, the module as the
    // producer gave it to llc.
    let arch = host().name();
    let entry =
        std::fs::read_to_string(world.out().join("kiln/llvm22/conformance/a/status.cpp.ll"))
            .unwrap();
    let (head, module) = entry.split_once("; llc-args: ").unwrap();
    assert_eq!(
        module,
        "-O0 -relocation-model=pic -filetype=obj\n; options: -std=c++23\n; program: cat\n; program: echo \"$1-$2\"\n; program: exit 3\n"
    );
    let directives: Vec<&str> = head.lines().filter(|line| line.starts_with(";@")).collect();
    assert_eq!(
        directives,
        [
            ";@ covers: kiln tests/conformance/a/status.cpp",
            format!(";@ target: {arch}").as_str(),
            ";@ llvm: 22",
            ";@ link: c++ m",
            ";@ args: x y",
            ";@ stdin: in",
            ";@ exit: 3",
            ";@ stdout: in",
            ";@ stdout: x-y",
        ]
    );
    assert!(head.contains("; source: kiln tests/conformance/a/status.cpp\n"), "{head}");
    assert!(head.contains("; producer-args: -std=c++23 "), "{head}");
    let entry =
        std::fs::read_to_string(world.out().join("kiln/llvm22/conformance/a/static.cpp.ll"))
            .unwrap();
    assert!(entry.contains(";@ link: c++ m no-pie\n"), "{entry}");
    let entry =
        std::fs::read_to_string(world.out().join("gcc-torture/llvm22/ieee/inf.c.ll")).unwrap();
    assert!(entry.contains(";@ covers: gcc-torture ieee/inf.c\n;"), "{entry}");
    assert!(entry.contains(";@ link: m\n"), "{entry}");
    assert!(
        entry.ends_with("; flags: -O0 -S -emit-llvm -w -fno-inline\n; program: exit 0\n"),
        "{entry}"
    );

    // The manifest: what made each entry.
    let manifest = Manifest::read(&world.out()).unwrap();
    let row = &manifest.rows["kiln/llvm22/conformance/a/tune.cpp.ll"];
    assert_eq!(row.llc_args, "-O0 -relocation-model=pic -filetype=obj -debugger-tune=gdb");
    assert_eq!(
        (row.source.as_str(), row.llvm, row.test.as_str()),
        (KILN, 22, "tests/conformance/a/tune.cpp")
    );
    assert_eq!(row.producer, "fake kiln version 0.1");
    assert_eq!(row.revision, "unknown");
    let row = &manifest.rows["gcc-torture/llvm22/flags.c.ll"];
    assert_eq!(row.producer_args, "-O0 -S -emit-llvm -w -fwrapv flags.c");
    assert_eq!(row.producer, "fake clang version 22.0.0");
    let text = std::fs::read(world.out().join("gcc-torture/llvm22/flags.c.ll")).unwrap();
    assert_eq!(row.bytes, text.len() as u64);
    assert_eq!(row.sha256, xtask::corpus::sha256::hex(&text));

    // Nothing of the capture's work is left but the shared directories.
    let work = capture::work_dir(&world.out());
    let left: Vec<String> = std::fs::read_dir(&work)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("run-"))
        .collect();
    assert_eq!(left, [""; 0]);

    // The corpus verifies: the manifest holds, and every entry passes
    // `irdiff --hybrid` with every linker.
    let verified = verify::verify(&world.verify_options()).unwrap();
    assert_eq!(verified.problems, [""; 0]);
    assert_eq!(verified.exit_status(), 0, "{}", verified.summary(true));
    assert_eq!(verified.entries, 9);
    assert_eq!(verified.runs.len(), 1);
    let (major, run) = &verified.runs[0];
    assert_eq!(*major, 22);
    assert_eq!(run.records.len(), 9 * 3, "{}", run.summary(true));

    // And it can be counted; the same corpus gives the same report.
    let text = census::render(&world.out(), 2).unwrap();
    assert_eq!(text, census::render(&world.out(), 1).unwrap());
    for wanted in [
        "| `kiln/llvm22` | 18 | 5 | 13 |",
        "| `gcc-torture/llvm22` | 12 | 4 | 8 |",
        "| `-O0 -relocation-model=pic -filetype=obj -debugger-tune=gdb` |  | 1 | no |",
        "1 of 9 entries were built by their producer with `llc` arguments `irdiff` does not repeat.",
        "- `tests/conformance/b/bad.cpp` (LLVM 22): `compile-fails`: ",
        "- `tests/conformance/b/mold.cpp` (LLVM 22): `not-reproducible`: ",
        "| modules | 4 | 5 |",
    ] {
        assert!(text.contains(wanted), "no '{wanted}' in:\n{text}");
    }
    assert!(!text.contains(&*world.scratch.join("").to_string_lossy()), "a path of this machine");
}

// spec/harness.md "corpus", "Incremental" and "Manifest": a second capture
// runs no producer and changes no byte; a changed test is captured again,
// a removed one leaves the corpus; `--force` makes everything again, to
// the same bytes; `--retry-skipped` tries the skipped again; `--limit`
// bounds a capture and removes nothing.
#[test]
fn incremental_and_reproducible() {
    let world = full_world("corpus-incremental");
    let first = world.capture();
    assert_eq!(first.entries, 9);
    let snapshot = world.snapshot();
    let calls = world.call_count();
    // 16 kiln tests reach kiln++ (two are skipped by their header), 10
    // torture tests reach clang (a multi-file one and one with special
    // flags do not).
    assert_eq!(calls, 16 + 10);

    let second = world.capture();
    assert_eq!(world.call_count(), calls, "nothing is made twice");
    assert_eq!(world.snapshot(), snapshot);
    let stats: Vec<(usize, usize, usize)> =
        second.groups.iter().map(|g| (g.captured, g.reused, g.skipped.values().sum())).collect();
    assert_eq!(stats, [(0, 5, 13), (0, 4, 8)]);

    // A changed test, a fixed test, a removed test.
    world.kiln_test(
        "conformance/a/hello.cpp",
        "//@ expect: run\n//@ stdout: hello again\n//; program: echo hello again\n",
    );
    world.torture_test("status.c", "//; program: exit 0\n");
    std::fs::remove_file(world.execute().join("out.c")).unwrap();
    let third = world.capture();
    assert_eq!(world.call_count(), calls + 2);
    let stats: Vec<(usize, usize, usize, usize)> = third
        .groups
        .iter()
        .map(|g| (g.captured, g.reused, g.skipped.values().sum(), g.removed))
        .collect();
    assert_eq!(stats, [(1, 4, 13, 0), (1, 3, 7, 1)]);
    assert!(!world.out().join("gcc-torture/llvm22/out.c.ll").exists());
    assert!(world.out().join("gcc-torture/llvm22/status.c.ll").is_file());
    assert_eq!(reasons(&world).get("gcc-torture/22/status.c"), None);
    let entry = std::fs::read_to_string(world.out().join("kiln/llvm22/conformance/a/hello.cpp.ll"))
        .unwrap();
    assert!(entry.contains(";@ stdout: hello again\n"), "{entry}");
    let verified = verify::verify(&world.verify_options()).unwrap();
    assert_eq!(verified.exit_status(), 0, "{}", verified.summary(true));

    // Again, from nothing but the tests: the same bytes.
    let snapshot = world.snapshot();
    let calls = world.call_count();
    let mut options = world.options();
    options.force = true;
    let forced = capture::capture(&options).unwrap();
    assert_eq!(world.call_count(), calls + 16 + 9);
    assert_eq!(world.snapshot(), snapshot);
    assert_eq!(
        forced.groups.iter().map(|g| (g.captured, g.reused)).collect::<Vec<_>>(),
        [(5, 0), (4, 0)]
    );

    // The skipped again.
    let calls = world.call_count();
    let mut options = world.options();
    options.retry_skipped = true;
    capture::capture(&options).unwrap();
    assert_eq!(world.call_count(), calls + 11 + 5);
    assert_eq!(world.snapshot(), snapshot);

    // A bounded capture into a new corpus takes the first tests by name,
    // and one into this corpus removes nothing.
    let mut options = world.options();
    options.limit = Some(2);
    let bounded = capture::capture(&options).unwrap();
    assert_eq!(bounded.groups.iter().map(|g| g.tests).collect::<Vec<_>>(), [2, 2]);
    assert_eq!(world.snapshot(), snapshot);
    options.out = world.scratch.join("small");
    let bounded = capture::capture(&options).unwrap();
    // (The first two torture tests by name are both skipped.)
    assert_eq!(bounded.entries, 2, "{}", bounded.summary(&options.out));
    let small = Manifest::read(&options.out).unwrap();
    assert_eq!(
        small.rows.keys().collect::<Vec<_>>(),
        ["kiln/llvm22/c/run/plain.c.ll", "kiln/llvm22/conformance/a/hello.cpp.ll"]
    );
    assert_eq!(Skips::read(&options.out).unwrap().rows.len(), 2);
    // The same test in another corpus directory is the same entry, with
    // the same row.
    let here = Manifest::read(&world.out()).unwrap();
    for (entry, row) in &small.rows {
        assert_eq!(Some(row), here.rows.get(entry));
    }
}

// spec/harness.md "corpus", "verify": every way a corpus can differ from
// its manifest is reported, with exit status 3, and no test is run for a
// `--manifest-only` check.
#[test]
fn verify_finds_damage() {
    let world = World::new("corpus-damage");
    for name in ["a.c", "b.c", "c.c"] {
        world.torture_test(name, "//; program: exit 0\n");
    }
    world.capture();
    let mut options = world.verify_options();
    options.manifest_only = true;
    let clean = verify::verify(&options).unwrap();
    assert_eq!((clean.entries, clean.problems.len(), clean.runs.len()), (3, 0, 0));
    assert_eq!(clean.exit_status(), 0);

    let dir = world.out().join("gcc-torture/llvm22");
    let mut text = std::fs::read_to_string(dir.join("a.c.ll")).unwrap();
    text.push_str("; one more line\n");
    std::fs::write(dir.join("a.c.ll"), text).unwrap();
    std::fs::remove_file(dir.join("b.c.ll")).unwrap();
    std::fs::write(dir.join("stray.ll"), "; not captured\n").unwrap();
    let damaged = verify::verify(&options).unwrap();
    assert_eq!(
        damaged.problems,
        [
            "gcc-torture/llvm22/a.c.ll: the file is not the one the manifest records",
            "gcc-torture/llvm22/b.c.ll: the file is missing",
            "gcc-torture/llvm22/stray.ll: not in the manifest",
        ]
    );
    assert_eq!(damaged.exit_status(), 3);
    assert!(
        damaged
            .summary(false)
            .contains("MANIFEST gcc-torture/llvm22/b.c.ll: the file is missing\n")
    );

    // The next capture repairs it: the damaged entries are made again.
    let repaired = world.capture();
    let captured: Vec<(&str, usize, usize)> =
        repaired.groups.iter().map(|g| (g.source.as_str(), g.captured, g.reused)).collect();
    assert_eq!(captured, [(KILN, 0, 0), (TORTURE, 2, 1)]);
    std::fs::remove_file(dir.join("stray.ll")).unwrap();
    assert_eq!(verify::verify(&options).unwrap().problems, [""; 0]);

    // No corpus at all is an error, not a pass.
    let mut nowhere = world.verify_options();
    nowhere.dir = world.scratch.join("nowhere");
    assert!(verify::verify(&nowhere).unwrap_err().contains("no corpus"));
}

// spec/harness.md "corpus", "Where things are": a source that is not on
// the machine is a note and no failure, whatever `HOD_REQUIRE_TOOLS`
// says; a missing oracle or required linker is a note, and a failure
// (status 3) when tools are required.
#[test]
fn missing_sources_and_tools() {
    let world = World::new("corpus-missing");
    world.torture_test("a.c", "//; program: exit 0\n");
    let mut options = world.options();
    options.kiln = world.scratch.join("no-kiln++");
    options.torture = world.scratch.join("no-suite");
    options.require_tools = true;
    let report = capture::capture(&options).unwrap();
    assert_eq!(report.exit_status(), 0, "{}", report.summary(&world.out()));
    assert_eq!(report.groups.len(), 0);
    let notes = report.notes.join("\n");
    assert!(notes.contains("the kiln source is skipped"), "{notes}");
    assert!(notes.contains("the gcc-torture source is skipped"), "{notes}");
    assert!(
        world.out().join("MANIFEST.tsv").is_file() && world.out().join("SKIPPED.tsv").is_file()
    );
    assert_eq!(report.entries, 0);

    // Both majors asked for, one installed; mold gone.
    std::fs::remove_file(world.scratch.join("bin/mold")).unwrap();
    let mut options = world.options();
    options.llvm = Vec::new();
    options.sources = vec![TORTURE];
    let report = capture::capture(&options).unwrap();
    assert_eq!(report.exit_status(), 0);
    assert_eq!(report.entries, 1);
    let notes = report.notes.join("\n");
    assert!(notes.contains("LLVM 19 (llc) not found"), "{notes}");
    assert!(notes.contains("linker 'mold' not found"), "{notes}");
    options.require_tools = true;
    let report = capture::capture(&options).unwrap();
    assert_eq!(report.exit_status(), 3);
    assert_eq!(report.missing_tools.len(), 2, "{:?}", report.missing_tools);
    assert!(report.summary(&world.out()).contains("MISSING LLVM 19 (llc) not found"));
}

// spec/harness.md "corpus", "Layout": each LLVM major has its own entries,
// made by its own tools, and `verify` runs each with its own oracle.
#[test]
fn one_group_per_llvm_major() {
    let world = World::new("corpus-majors");
    world.llvm(19);
    world.tool("ld.lld-19", &fake_linker("lld"));
    world.kiln_test("conformance/a/hello.cpp", "//@ expect: run\n//; program: echo hello\n");
    world.kiln_test(
        "conformance/a/new.cpp",
        "//@ expect: run\n//@ requires: clang>=20\n//; program: exit 0\n",
    );
    world.torture_test("a.c", "//; program: exit 0\n");
    let mut options = world.options();
    options.llvm = Vec::new();
    let report = capture::capture(&options).unwrap();
    assert_eq!(report.exit_status(), 0);
    assert_eq!(
        entries(&world),
        [
            "gcc-torture/llvm19/a.c.ll",
            "gcc-torture/llvm22/a.c.ll",
            "kiln/llvm19/conformance/a/hello.cpp.ll",
            "kiln/llvm22/conformance/a/hello.cpp.ll",
            "kiln/llvm22/conformance/a/new.cpp.ll",
        ]
    );
    let wanted: BTreeMap<String, String> =
        [("kiln/19/tests/conformance/a/new.cpp".to_owned(), "requires".to_owned())].into();
    assert_eq!(reasons(&world), wanted);
    let entry = std::fs::read_to_string(world.out().join("kiln/llvm19/conformance/a/hello.cpp.ll"))
        .unwrap();
    assert!(entry.contains(";@ llvm: 19\n"), "{entry}");

    let verified = verify::verify(&world.verify_options()).unwrap();
    assert_eq!(verified.exit_status(), 0, "{}", verified.summary(true));
    let runs: Vec<(u32, usize)> =
        verified.runs.iter().map(|(major, run)| (*major, run.records.len())).collect();
    assert_eq!(runs, [(22, 3 * 3), (19, 2 * 3)]);

    // One major alone leaves the other's entries as they are.
    let snapshot = world.snapshot();
    let mut options = world.options();
    options.llvm = vec![19];
    options.force = true;
    capture::capture(&options).unwrap();
    assert_eq!(world.snapshot(), snapshot);
}
