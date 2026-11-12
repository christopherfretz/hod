//! The GCC torture suite (`gcc.c-torture/execute`) as a source of IR:
//! each test is one C file whose program must exit with status 0; clang
//! compiles it to a module. Contract: `spec/harness.md` "corpus", "The
//! gcc-torture source".

use std::fs;
use std::path::{Path, PathBuf};

use super::entry::Unfit;
use crate::irdiff::directives::Arch;

/// The suite's directory in a checkout of GCC's sources.
pub const EXECUTE_DIR: &str = "gcc/testsuite/gcc.c-torture/execute";
/// Where the suite comes from, and the tag the capture is pinned to: the
/// one kiln's `scripts/fetch-gcc-tests.sh` pins.
pub const REPOSITORY: &str = "https://github.com/gcc-mirror/gcc.git";
pub const TAG: &str = "releases/gcc-16.1.0";

/// One test of the suite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TortureTest {
    /// The path below `execute/`, with `/`: `20000112-1.c`, `ieee/fp-cmp-1.c`.
    pub name: String,
    pub path: PathBuf,
    /// Why the test is never captured, if it is known from where it is.
    pub unfit: Option<Unfit>,
}

/// Finds the tests of `execute` (the suite's directory): the `*.c` files
/// of the directory itself and of `ieee/`, and those of `builtins/`, which
/// are built from several files and are found only to be counted. Sorted
/// by name.
pub fn discover(execute: &Path) -> Result<Vec<TortureTest>, String> {
    let mut found = Vec::new();
    for sub in ["", "ieee", "builtins"] {
        let dir = if sub.is_empty() { execute.to_path_buf() } else { execute.join(sub) };
        if !dir.is_dir() {
            if sub.is_empty() {
                return Err(format!("'{}' is not a directory", dir.display()));
            }
            continue;
        }
        let failed = |e: std::io::Error| format!("cannot read '{}': {e}", dir.display());
        for entry in fs::read_dir(&dir).map_err(failed)? {
            let path = entry.map_err(failed)?.path();
            let Some(file) = path.file_name().and_then(|name| name.to_str()) else { continue };
            let is_c = path.extension().is_some_and(|extension| extension == "c");
            if !is_c || file.starts_with('.') || !path.is_file() {
                continue;
            }
            // `builtins/x.c` is built with `builtins/x-lib.c` and the
            // directory's `lib/`: the `-lib.c` files are not tests.
            if sub == "builtins" && file.ends_with("-lib.c") {
                continue;
            }
            let name = if sub.is_empty() { file.to_owned() } else { format!("{sub}/{file}") };
            let unfit = (sub == "builtins").then(|| {
                Unfit::new("multi-file", "built from the test, its -lib.c and builtins/lib/")
            });
            found.push(TortureTest { name, path, unfit });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// One `dg-options` or `dg-additional-options` directive of a test.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DgOptions {
    flags: Vec<String>,
    /// The text of the selector (`target ...`), if the directive has one.
    selector: Option<String>,
}

/// The `{ dg-options "..." [{ selector }] }` and `dg-additional-options`
/// directives of `source`, in order.
fn dg_options(source: &str) -> Vec<DgOptions> {
    let mut found = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find("dg-") {
        rest = &rest[at + 3..];
        let Some(after) =
            rest.strip_prefix("options").or_else(|| rest.strip_prefix("additional-options"))
        else {
            continue;
        };
        let after = after.trim_start_matches([' ', '\t']);
        let Some(quoted) = after.strip_prefix('"') else { continue };
        let Some(end) = quoted.find('"') else { continue };
        let flags = quoted[..end].split_whitespace().map(str::to_owned).collect();
        // What follows the string up to the directive's closing brace is
        // the selector, itself in braces.
        let tail = quoted[end + 1..].trim_start_matches([' ', '\t']);
        let selector = tail.strip_prefix('{').map(|inner| {
            let mut depth = 1usize;
            let mut close = inner.len();
            for (index, c) in inner.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            close = index;
                            break;
                        }
                    }
                    '\n' => {
                        close = index;
                        break;
                    }
                    _ => {}
                }
            }
            inner[..close].trim().to_owned()
        });
        found.push(DgOptions { flags, selector });
        rest = &quoted[end + 1..];
    }
    found
}

/// The flags a test's `.x` file adds without a condition: the quoted words
/// of its `set additional_flags` and `lappend additional_flags` lines that
/// are not indented.
fn x_file_flags(text: &str) -> Vec<String> {
    let mut flags = Vec::new();
    for line in text.lines() {
        let Some(rest) = line
            .strip_prefix("lappend additional_flags")
            .or_else(|| line.strip_prefix("set additional_flags"))
        else {
            continue;
        };
        for (index, part) in rest.split('"').enumerate() {
            if index % 2 == 1 {
                flags.extend(part.split_whitespace().map(str::to_owned));
            }
        }
    }
    flags
}

/// What becomes of one flag a test asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flag {
    /// Given to clang.
    Pass,
    /// Left out: it selects optimizations or warnings, which a capture at
    /// `-O0 -w` does not have.
    Drop,
    /// The test needs something this capture does not do.
    Special,
}

/// What GCC's `-fpermissive` is for clang: the diagnostics GCC then
/// only warns about, which clang makes errors by default, are not errors.
pub const PERMISSIVE: [&str; 4] = [
    "-Wno-error=implicit-function-declaration",
    "-Wno-error=implicit-int",
    "-Wno-error=int-conversion",
    "-Wno-error=incompatible-pointer-types",
];

fn classify(flag: &str) -> Flag {
    const PASS: [&str; 15] = [
        "-std=",
        "-D",
        "-U",
        "-fwrapv",
        "-fno-strict-overflow",
        "-fno-strict-aliasing",
        "-fgnu89-inline",
        "-fno-common",
        "-fno-builtin",
        "-fno-inline",
        "-fomit-frame-pointer",
        "-fno-trapping-math",
        "-ffast-math",
        "-finput-charset=",
        "-fpic",
    ];
    const DROP: [&str; 14] = [
        "-O",
        "-w",
        "-pedantic",
        "-ftree-",
        "-fno-tree-",
        "-fno-ipa-",
        "-fno-ira-",
        "-fmodulo-sched",
        "-fno-vect-cost-model",
        "-foptimize-strlen",
        "-fno-early-inlining",
        "-fno-dce",
        "-fexpensive-optimizations",
        "-fno-ivopts",
    ];
    if ["-Wl,", "-Wa,", "-Wp,"].iter().any(|prefix| flag.starts_with(prefix)) {
        Flag::Special
    } else if flag.starts_with("-W") || DROP.iter().any(|prefix| flag.starts_with(prefix)) {
        Flag::Drop
    } else if PASS.iter().any(|prefix| flag.starts_with(prefix)) {
        Flag::Pass
    } else {
        Flag::Special
    }
}

/// The flags clang gets for a test besides `-O0 -S -emit-llvm -w`, from
/// the test's `dg-options` and `dg-additional-options`, its `.x` file
/// (`x_file`) and its directory. A directive with a selector is ignored,
/// unless the selector names `host`: then the test needs flags of its own
/// on this machine, and is not captured.
pub fn flags(
    name: &str,
    source: &str,
    x_file: Option<&str>,
    host: Arch,
) -> Result<Vec<String>, Unfit> {
    let mut wanted = Vec::new();
    // `ieee/ieee.exp` gives every test of the directory this flag.
    if name.starts_with("ieee/") {
        wanted.push("-fno-inline".to_owned());
    }
    wanted.extend(x_file.map(x_file_flags).unwrap_or_default());
    for directive in dg_options(source) {
        match &directive.selector {
            None => wanted.extend(directive.flags),
            Some(selector) if selector.contains(host.name()) => {
                let detail = format!(
                    "options for this target: \"{}\" {{ {selector} }}",
                    directive.flags.join(" ")
                );
                return Err(Unfit::new("special-flags", detail));
            }
            Some(_) => {}
        }
    }
    let mut kept: Vec<String> = Vec::new();
    for flag in wanted {
        if flag == "-fpermissive" {
            for flag in PERMISSIVE {
                if !kept.iter().any(|seen| seen == flag) {
                    kept.push(flag.to_owned());
                }
            }
            continue;
        }
        match classify(&flag) {
            Flag::Pass if !kept.contains(&flag) => kept.push(flag),
            Flag::Pass | Flag::Drop => {}
            Flag::Special => {
                return Err(Unfit::new("special-flags", format!("the test asks for {flag}")));
            }
        }
    }
    Ok(kept)
}

/// clang's arguments for a test, run in the suite's directory: the module
/// is written to `module`.
pub fn clang_args(name: &str, flags: &[String]) -> Vec<String> {
    let mut args: Vec<String> =
        ["-O0", "-S", "-emit-llvm", "-w"].iter().map(|arg| (*arg).to_owned()).collect();
    args.extend(flags.iter().cloned());
    args.push(name.to_owned());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::Scratch;

    // spec/harness.md "corpus", "The gcc-torture source": which files are
    // tests.
    #[test]
    fn discovery() {
        let scratch = Scratch::new("corpus-torture-discover");
        for file in [
            "b.c",
            "a.c",
            "a.x",
            "notes.h",
            ".hidden.c",
            "ieee/fp.c",
            "ieee/fp.x",
            "builtins/abs.c",
            "builtins/abs-lib.c",
            "builtins/lib/main.c",
            "other/x.c",
        ] {
            let path = scratch.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        let found = discover(scratch.path()).unwrap();
        let names: Vec<(&str, Option<&str>)> = found
            .iter()
            .map(|test| (test.name.as_str(), test.unfit.as_ref().map(|unfit| unfit.reason)))
            .collect();
        assert_eq!(
            names,
            [
                ("a.c", None),
                ("b.c", None),
                ("builtins/abs.c", Some("multi-file")),
                ("ieee/fp.c", None)
            ]
        );
        assert_eq!(found[3].path, scratch.join("ieee/fp.c"));
        assert!(discover(&scratch.join("missing")).is_err());
    }

    // "The gcc-torture source": the flags of a test.
    #[test]
    fn flags_of_a_test() {
        let host = Arch::Aarch64;
        let of = |source: &str| flags("t.c", source, None, host);
        assert_eq!(of("int main(void) { return 0; }").unwrap(), [""; 0]);
        assert_eq!(
            of("/* { dg-options \"-std=gnu89 -O2 -Wno-psabi\" } */\n\
                /* { dg-additional-options \"-fwrapv -fno-tree-vrp -DX=1\" } */\n")
            .unwrap(),
            ["-std=gnu89", "-fwrapv", "-DX=1"]
        );
        // A flag is given once.
        assert_eq!(
            of("/* { dg-options \"-fwrapv\" } */ /* { dg-additional-options \"-fwrapv -w\" } */")
                .unwrap(),
            ["-fwrapv"]
        );
        // A selector that does not name this machine: ignored. One that
        // does: the test is not captured.
        assert_eq!(
            of("/* { dg-additional-options \"-mno-mmx\" { target { x86_64-*-* i?86-*-* } } } */")
                .unwrap(),
            [""; 0]
        );
        assert_eq!(
            of("/* { dg-options \"-fpermissive\" { target { ! signal } } } */").unwrap(),
            [""; 0]
        );
        // GCC's -fpermissive, in clang's words.
        let permissive =
            of("/* { dg-additional-options \"-std=gnu17 -fpermissive -fpermissive\" } */").unwrap();
        assert_eq!(permissive[0], "-std=gnu17");
        assert_eq!(permissive[1..], PERMISSIVE);
        let unfit = of("/* { dg-options \"-mcpu=x\" { target aarch64*-*-* } } */\n").unwrap_err();
        assert_eq!(unfit.reason, "special-flags");
        assert_eq!(unfit.detail, "options for this target: \"-mcpu=x\" { target aarch64*-*-* }");
        assert_eq!(
            flags("t.c", "/* { dg-options \"-O\" { target x86_64-*-* } } */", None, Arch::X86_64)
                .unwrap_err()
                .reason,
            "special-flags"
        );
        // What the capture cannot honour.
        for flag in ["-mtune=generic", "-Wl,-u,_printf_float", "-fnon-call-exceptions", "-pthread"]
        {
            let unfit = of(&format!("/* {{ dg-options \"-O2 {flag}\" }} */")).unwrap_err();
            assert_eq!(
                (unfit.reason, unfit.detail),
                ("special-flags", format!("the test asks for {flag}"))
            );
        }
        // Not a directive.
        assert_eq!(of("/* dg-options without a string */ int dg-optional;").unwrap(), [""; 0]);

        // The directory's flag and the `.x` file's, before the test's own.
        let x_file = "# a comment\nlappend additional_flags \"-std=gnu17\"\n\
                      if [istarget \"avr-*-*\"] {\n    lappend additional_flags \"-mieee\"\n}\n\
                      return 0\n";
        assert_eq!(
            flags("ieee/fp.c", "/* { dg-options \"-fwrapv\" } */", Some(x_file), host).unwrap(),
            ["-fno-inline", "-std=gnu17", "-fwrapv"]
        );
        let odd = "lappend additional_flags \"-fno-ivopts\" \"-Wl,-u,_printf_float\"\n";
        assert_eq!(flags("a.c", "", Some(odd), host).unwrap_err().reason, "special-flags");
        assert_eq!(x_file_flags("set additional_flags \"-a -b\" \"-c\"\n"), ["-a", "-b", "-c"]);
    }

    // "The gcc-torture source": clang's command line.
    #[test]
    fn command_line() {
        assert_eq!(
            clang_args("ieee/fp.c", &["-std=gnu89".to_owned()]),
            ["-O0", "-S", "-emit-llvm", "-w", "-std=gnu89", "ieee/fp.c"]
        );
        assert_eq!(clang_args("a.c", &[]), ["-O0", "-S", "-emit-llvm", "-w", "a.c"]);
    }
}
