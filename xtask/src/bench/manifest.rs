//! The benchmark inputs: the manifest that lists them and the production
//! of their IR. Contract: `spec/harness.md` "bench", "Manifest" and
//! "Inputs".

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::record::{Class, fnv1a64, write_replacing};
use crate::irdiff::process::{self, End, Limits};
use crate::irdiff::tools::{Llvm, Search};

/// What the harness adds to a compile itself; a manifest may not.
const RESERVED_FLAGS: [&str; 2] = ["-o", "-g"];

/// The limits of one compile that produces an input.
pub const PRODUCER_LIMITS: Limits = Limits { time: Duration::from_mins(3), memory_kib: 8 << 20 };

/// A source's language, by its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    C,
    Cxx,
}

impl Lang {
    fn of(source: &str) -> Option<Self> {
        match Path::new(source).extension()?.to_str()? {
            "c" => Some(Self::C),
            "cpp" | "cc" | "cxx" => Some(Self::Cxx),
            _ => None,
        }
    }
}

/// A section of the manifest: one source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub class: Class,
    /// The file, relative to `bench/src/`.
    pub source: String,
    pub flags: Vec<String>,
}

/// `bench/manifest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// The flags of every compile.
    pub common: Vec<String>,
    pub entries: Vec<Entry>,
}

/// One input: an entry, with or without debug information.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    /// The entry's name, with `-g` appended for the debug variant.
    pub id: String,
    pub class: Class,
    pub debug: bool,
    pub source: String,
    pub lang: Lang,
    /// The compile's arguments between the compiler and `-o`.
    pub args: Vec<String>,
}

fn valid_name(name: &str) -> bool {
    let plain = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    !name.is_empty()
        && name.bytes().all(|b| plain(b) || b == b'-')
        && plain(name.as_bytes()[0])
        && !name.ends_with('-')
        && !name.ends_with("-g")
}

/// A section being read.
struct Open {
    line: usize,
    name: String,
    class: Option<Class>,
    source: Option<String>,
    flags: Option<Vec<String>>,
}

impl Open {
    /// Ends a section: an entry, or what it lacks.
    fn close(open: Option<Self>, entries: &mut Vec<Entry>, errors: &mut Vec<String>) {
        let Some(open) = open else { return };
        match (open.class, open.source) {
            (Some(class), Some(source)) => {
                let flags = open.flags.unwrap_or_default();
                entries.push(Entry { name: open.name, class, source, flags });
            }
            (class, source) => {
                for (missing, key) in [(class.is_none(), "class"), (source.is_none(), "source")] {
                    if missing {
                        errors.push(format!("line {}: [{}] has no '{key}'", open.line, open.name));
                    }
                }
            }
        }
    }
}

impl Manifest {
    /// Reads the manifest's text. Every error is reported, with its line.
    pub fn parse(text: &str) -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();
        let mut common: Option<Vec<String>> = None;
        let mut entries: Vec<Entry> = Vec::new();
        let mut open: Option<Open> = None;
        for (index, raw) in text.lines().enumerate() {
            let number = index + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
                Open::close(open.take(), &mut entries, &mut errors);
                if !valid_name(name) {
                    errors.push(format!(
                        "line {number}: invalid name '{name}': lower-case letters, digits and '-', \
                         not ending in '-g'"
                    ));
                } else if entries.iter().any(|entry| entry.name == name) {
                    errors.push(format!("line {number}: a second [{name}]"));
                }
                // An entry with a bad name is still read, so that its keys
                // are checked; it cannot be used, because there are errors.
                open = Some(Open {
                    line: number,
                    name: name.to_owned(),
                    class: None,
                    source: None,
                    flags: None,
                });
                continue;
            }
            let Some((key, value)) = line.split_once('=').map(|(k, v)| (k.trim(), v.trim())) else {
                errors.push(format!("line {number}: expected '[name]' or 'key = value'"));
                continue;
            };
            let words = || -> Vec<String> { value.split_whitespace().map(str::to_owned).collect() };
            let reserved = words().into_iter().find(|word| RESERVED_FLAGS.contains(&word.as_str()));
            let flag_list = |slot: &mut Option<Vec<String>>, errors: &mut Vec<String>| {
                if slot.is_some() {
                    errors.push(format!("line {number}: a second '{key}'"));
                } else if let Some(word) = &reserved {
                    errors.push(format!("line {number}: '{word}' is added by the harness"));
                }
                *slot = Some(words());
            };
            match (&mut open, key) {
                (None, "common") => flag_list(&mut common, &mut errors),
                (None, _) => errors.push(format!("line {number}: '{key}' outside a section")),
                (Some(_), _) if value.is_empty() && key != "flags" => {
                    errors.push(format!("line {number}: '{key}' has no value"));
                }
                (Some(entry), "class") => match Class::parse(value) {
                    _ if entry.class.is_some() => {
                        errors.push(format!("line {number}: a second 'class'"));
                    }
                    Some(class) => entry.class = Some(class),
                    None => {
                        errors.push(format!(
                            "line {number}: unknown class '{value}': small, typical, large or pathological"
                        ));
                        // Not also reported as missing.
                        entry.class = Some(Class::Small);
                    }
                },
                (Some(entry), "source") => {
                    let inside = !value.starts_with('/')
                        && value.split('/').all(|part| part != ".." && !part.is_empty());
                    if entry.source.is_some() {
                        errors.push(format!("line {number}: a second 'source'"));
                    } else if !inside {
                        errors.push(format!(
                            "line {number}: source '{value}' is not a path inside bench/src"
                        ));
                    } else if Lang::of(value).is_none() {
                        errors.push(format!(
                            "line {number}: source '{value}' is not C (.c) or C++ (.cpp, .cc, .cxx)"
                        ));
                    }
                    entry.source = Some(value.to_owned());
                }
                (Some(entry), "flags") => flag_list(&mut entry.flags, &mut errors),
                (Some(_), _) => errors.push(format!("line {number}: unknown key '{key}'")),
            }
        }
        Open::close(open.take(), &mut entries, &mut errors);
        if common.is_none() {
            errors.push("no 'common' line before the first section".to_owned());
        }
        if entries.is_empty() && errors.is_empty() {
            errors.push("no input".to_owned());
        }
        if errors.is_empty() {
            Ok(Self { common: common.unwrap_or_default(), entries })
        } else {
            Err(errors)
        }
    }

    /// Reads `path`. The error is one message.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
        Self::parse(&text).map_err(|errors| format!("'{}': {}", path.display(), errors.join("; ")))
    }

    /// The inputs, in the manifest's order: each entry without, then with,
    /// debug information.
    pub fn inputs(&self) -> Vec<Input> {
        let mut inputs = Vec::new();
        for entry in &self.entries {
            for debug in [false, true] {
                let mut args = self.common.clone();
                args.extend(entry.flags.iter().cloned());
                if debug {
                    args.push("-g".to_owned());
                }
                inputs.push(Input {
                    id: if debug { format!("{}-g", entry.name) } else { entry.name.clone() },
                    class: entry.class,
                    debug,
                    source: entry.source.clone(),
                    // Checked when the manifest was read.
                    lang: Lang::of(&entry.source).unwrap_or(Lang::Cxx),
                    args,
                });
            }
        }
        inputs
    }
}

/// The compilers that turn sources into IR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Producer {
    /// The name of the inputs' directory: `clang<major>` or `kiln`.
    pub tag: String,
    pub c: Option<PathBuf>,
    pub cxx: Option<PathBuf>,
    /// The first line of the C++ compiler's `--version` (the C compiler's
    /// if there is no C++ compiler).
    pub version: String,
}

/// The first line a tool prints for `--version`.
pub fn version_line(tool: &Path) -> Result<String, String> {
    let limits = Limits { time: Duration::from_secs(20), memory_kib: 4 << 20 };
    let output = process::run(
        tool,
        |c| {
            c.arg("--version");
        },
        b"",
        limits,
    )?;
    let text = String::from_utf8_lossy(&output.stdout);
    match (output.end, text.lines().map(str::trim).find(|line| !line.is_empty())) {
        (End::Exit(0), Some(line)) => Ok(line.to_owned()),
        _ => Err(format!("'{} --version' printed no version", tool.display())),
    }
}

impl Producer {
    /// clang of the oracle's LLVM. `clang` and `clang++` are looked for
    /// beside `llc` under its own naming (`llc-22` has `clang-22`), then
    /// as `<name>-<major>` on `PATH`, then in `<llvm root>/llvm-<major>/bin`.
    pub fn clang(llvm: &Llvm, search: &Search) -> Result<Self, String> {
        let beside = llvm.llc.parent().unwrap_or(Path::new("."));
        let suffix = if llvm.llc.file_name().is_some_and(|name| name == "llc") {
            String::new()
        } else {
            format!("-{}", llvm.major)
        };
        let find = |name: &str| {
            let versioned = format!("{name}-{}", llvm.major);
            let candidates = [beside.join(format!("{name}{suffix}"))]
                .into_iter()
                .chain(std::env::split_paths(&search.path).map(|dir| dir.join(&versioned)))
                .chain([search.llvm_root.join(format!("llvm-{}/bin/{name}", llvm.major))]);
            candidates.into_iter().find(|path| path.is_file())
        };
        let (c, cxx) = (find("clang"), find("clang++"));
        let Some(any) = cxx.as_ref().or(c.as_ref()) else {
            return Err(format!("no clang of LLVM {} found to produce the inputs", llvm.major));
        };
        let version = version_line(any)?;
        Ok(Self { tag: format!("clang{}", llvm.major), c, cxx, version })
    }

    /// kiln: `kiln++` for C++, and the `kiln` beside it for C.
    pub fn kiln(kilnxx: &Path) -> Result<Self, String> {
        if !kilnxx.is_file() {
            return Err(format!("'{}' is not a file", kilnxx.display()));
        }
        let kilnxx = std::path::absolute(kilnxx).unwrap_or_else(|_| kilnxx.to_path_buf());
        let c = kilnxx.file_name().and_then(|name| name.to_str()).and_then(|name| {
            let beside = kilnxx.with_file_name(name.strip_suffix("++")?);
            beside.is_file().then_some(beside)
        });
        let version = version_line(&kilnxx)?;
        Ok(Self { tag: "kiln".to_owned(), c, cxx: Some(kilnxx), version })
    }

    fn compiler(&self, lang: Lang) -> Result<&Path, String> {
        let (found, what) = match lang {
            Lang::C => (&self.c, "C"),
            Lang::Cxx => (&self.cxx, "C++"),
        };
        found
            .as_deref()
            .ok_or_else(|| format!("the producer ({}) has no {what} compiler", self.tag))
    }
}

/// An input's IR and what identifies it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generated {
    pub path: PathBuf,
    pub ir_bytes: u64,
    /// Lines that start with `define `.
    pub functions: u64,
    pub ir_hash: String,
    /// Whether the compiler was run (else the file was up to date).
    pub fresh: bool,
}

/// The size, the function count and the hash of an IR file's bytes.
pub fn describe(ir: &[u8]) -> (u64, u64, String) {
    let functions = ir.split(|&b| b == b'\n').filter(|line| line.starts_with(b"define ")).count();
    (ir.len() as u64, functions as u64, fnv1a64(ir))
}

/// Produces `<out_dir>/<id>.ll`, unless it is up to date and `force` is
/// off. Up to date means: made by the same compiler version, with the same
/// arguments, from the same source bytes (recorded in `<id>.stamp`).
pub fn generate(
    input: &Input,
    producer: &Producer,
    src_dir: &Path,
    out_dir: &Path,
    force: bool,
    limits: Limits,
) -> Result<Generated, String> {
    let compiler = producer.compiler(input.lang)?;
    let source = src_dir.join(&input.source);
    let source_bytes =
        std::fs::read(&source).map_err(|e| format!("cannot read '{}': {e}", source.display()))?;
    let mut identity = format!("{}\0{}\0", producer.version, input.args.join("\0")).into_bytes();
    identity.extend_from_slice(&source_bytes);
    let key = fnv1a64(&identity);

    let path = out_dir.join(format!("{}.ll", input.id));
    let stamp = out_dir.join(format!("{}.stamp", input.id));
    if !force && let Some(known) = read_stamp(&stamp, &key, &path) {
        return Ok(known);
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("cannot create '{}': {e}", out_dir.display()))?;
    let _ = std::fs::remove_file(&stamp);
    let temporary = out_dir.join(format!("{}.ll.tmp-{}", input.id, std::process::id()));
    let output = process::run(
        compiler,
        |c| {
            c.args(&input.args).arg("-o").arg(&temporary).arg(&input.source).current_dir(src_dir);
        },
        b"",
        limits,
    )?;
    let ir = match output.end {
        End::Exit(0) => {
            std::fs::read(&temporary).map_err(|_| "the compiler wrote no IR".to_owned())
        }
        End::Exit(status) => Err(format!("the compiler exited with status {status}")),
        End::Signal(signal) => Err(format!("the compiler was terminated by signal {signal}")),
        End::TimedOut => Err(format!("the compiler timed out after {} s", limits.time.as_secs())),
    };
    let ir = match ir {
        Ok(ir) if !ir.is_empty() => ir,
        other => {
            let _ = std::fs::remove_file(&temporary);
            let why = other.err().unwrap_or_else(|| "the compiler wrote no IR".to_owned());
            let said = output.first_error_line();
            return Err(if said.is_empty() { why } else { format!("{why}: {said}") });
        }
    };
    std::fs::rename(&temporary, &path)
        .map_err(|e| format!("cannot write '{}': {e}", path.display()))?;
    let (ir_bytes, functions, ir_hash) = describe(&ir);
    write_replacing(
        &stamp,
        &format!("key\t{key}\nir_bytes\t{ir_bytes}\nfunctions\t{functions}\nir_hash\t{ir_hash}\n"),
    )?;
    Ok(Generated { path, ir_bytes, functions, ir_hash, fresh: true })
}

/// What the stamp says of the IR at `path`, if the stamp is for `key` and
/// the file is there with the recorded size.
fn read_stamp(stamp: &Path, key: &str, path: &Path) -> Option<Generated> {
    let text = std::fs::read_to_string(stamp).ok()?;
    let value = |name: &str| {
        text.lines()
            .find_map(|line| line.split_once('\t').filter(|(k, _)| *k == name).map(|(_, v)| v))
    };
    let ir_bytes: u64 = value("ir_bytes")?.parse().ok()?;
    let fits = value("key")? == key && std::fs::metadata(path).ok()?.len() == ir_bytes;
    fits.then_some(())?;
    Some(Generated {
        path: path.to_path_buf(),
        ir_bytes,
        functions: value("functions")?.parse().ok()?,
        ir_hash: value("ir_hash")?.to_owned(),
        fresh: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::Scratch;

    const GOOD: &str = "\
# a comment
common = -O0 -S -emit-llvm

[small-c]
class = small
source = small.c
flags = -std=c17

  [stl-heavy]
  source = deep/stl_heavy.cpp
  class = large
";

    // spec/harness.md "bench", "Manifest": sections, keys, and the two
    // inputs of each entry with their compile arguments.
    #[test]
    fn manifest_is_parsed() {
        let manifest = Manifest::parse(GOOD).unwrap();
        assert_eq!(manifest.common, ["-O0", "-S", "-emit-llvm"]);
        assert_eq!(
            manifest.entries,
            [
                Entry {
                    name: "small-c".into(),
                    class: Class::Small,
                    source: "small.c".into(),
                    flags: vec!["-std=c17".into()],
                },
                Entry {
                    name: "stl-heavy".into(),
                    class: Class::Large,
                    source: "deep/stl_heavy.cpp".into(),
                    flags: vec![],
                },
            ]
        );
        let inputs = manifest.inputs();
        let ids: Vec<&str> = inputs.iter().map(|input| input.id.as_str()).collect();
        assert_eq!(ids, ["small-c", "small-c-g", "stl-heavy", "stl-heavy-g"]);
        assert_eq!(inputs[0].args, ["-O0", "-S", "-emit-llvm", "-std=c17"]);
        assert_eq!(inputs[1].args, ["-O0", "-S", "-emit-llvm", "-std=c17", "-g"]);
        assert_eq!(
            (inputs[1].debug, inputs[1].lang, inputs[1].class),
            (true, Lang::C, Class::Small)
        );
        assert_eq!(
            (inputs[2].debug, inputs[2].lang, inputs[2].class),
            (false, Lang::Cxx, Class::Large)
        );
        assert_eq!(inputs[3].args, ["-O0", "-S", "-emit-llvm", "-g"]);
    }

    // "Manifest": every error is reported, with its line.
    #[test]
    fn manifest_errors() {
        let errors = |text: &str| Manifest::parse(text).unwrap_err();
        assert_eq!(errors(""), ["no 'common' line before the first section"]);
        assert_eq!(errors("common = -O0\n"), ["no input"]);
        assert_eq!(
            errors(
                "common = -O0 -g\ncommon = -O1\nclass = small\nwhat\n[a]\nclass = small\nsource = a.c\n"
            ),
            [
                "line 1: '-g' is added by the harness",
                "line 2: a second 'common'",
                "line 3: 'class' outside a section",
                "line 4: expected '[name]' or 'key = value'",
            ]
        );
        assert_eq!(
            errors(
                "common =\n[Bad]\nclass = small\nsource = a.c\n[x-g]\nclass = small\nsource = a.c\n\
                 [a]\nclass = small\nsource = a.c\n[a]\nclass = small\nsource = a.c\n"
            ),
            [
                "line 2: invalid name 'Bad': lower-case letters, digits and '-', not ending in '-g'",
                "line 5: invalid name 'x-g': lower-case letters, digits and '-', not ending in '-g'",
                "line 11: a second [a]",
            ]
        );
        assert_eq!(
            errors(
                "common = -S\n[a]\nclass = huge\nclass = small\nsource = ../a.c\nsource = b.c\n\
                 flags = -o x\nflags = -O1\ncolour = red\nsource =\n[b]\nsource = b.rs\n[c]\n\
                 [d]\nsource = /abs/d.c\nclass = small\n"
            ),
            [
                "line 3: unknown class 'huge': small, typical, large or pathological",
                "line 4: a second 'class'",
                "line 5: source '../a.c' is not a path inside bench/src",
                "line 6: a second 'source'",
                "line 7: '-o' is added by the harness",
                "line 8: a second 'flags'",
                "line 9: unknown key 'colour'",
                "line 10: 'source' has no value",
                "line 12: source 'b.rs' is not C (.c) or C++ (.cpp, .cc, .cxx)",
                "line 11: [b] has no 'class'",
                "line 13: [c] has no 'class'",
                "line 13: [c] has no 'source'",
                "line 15: source '/abs/d.c' is not a path inside bench/src",
            ]
        );
        let scratch = Scratch::new("bench-manifest");
        let path = scratch.write("manifest", "common = -S\n[a]\n");
        let message = Manifest::read(&path).unwrap_err();
        assert!(
            message.ends_with("manifest': line 2: [a] has no 'class'; line 2: [a] has no 'source'"),
            "{message}"
        );
        assert!(Manifest::read(&scratch.join("absent")).unwrap_err().starts_with("cannot read '"));
    }

    // "Manifest": the repository's own manifest is valid, names sources
    // that exist, and covers every class with and without debug
    // information.
    #[test]
    fn the_repository_manifest() {
        let bench = crate::irdiff::workspace_root().join("bench");
        let manifest = Manifest::read(&bench.join("manifest")).unwrap();
        for entry in &manifest.entries {
            assert!(bench.join("src").join(&entry.source).is_file(), "{}", entry.source);
        }
        let inputs = manifest.inputs();
        for class in [Class::Small, Class::Typical, Class::Large, Class::Pathological] {
            for debug in [false, true] {
                assert!(
                    inputs.iter().any(|i| i.class == class && i.debug == debug),
                    "{class:?} {debug}"
                );
            }
        }
        // Every source of bench/src is an input.
        for file in std::fs::read_dir(bench.join("src")).unwrap() {
            let name = file.unwrap().file_name().into_string().unwrap();
            assert!(
                manifest.entries.iter().any(|entry| entry.source == name),
                "{name} is not in the manifest"
            );
        }
    }

    /// A stand-in compiler: writes its arguments and the source as "IR",
    /// counts its runs in `<its own name>.runs`, and fails on request.
    const FAKE_COMPILER: &str = r#"PATH=/usr/bin:/bin
if [ "$1" = --version ]; then echo; echo "fake clang version 22.0"; echo "Target: none"; exit 0; fi
echo run >> "$0.runs"
out= src= args=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -*) args="$args $1" ;;
    *) src=$1 ;;
  esac
  shift
done
if grep -q 'fail' "$src"; then echo "fake: error: cannot compile $src" >&2; exit 1; fi
if grep -q 'silent' "$src"; then exit 0; fi
if grep -q 'hang' "$src"; then exec sleep 60; fi
{ echo "; args$args"; echo "; cwd $(pwd)"; echo 'define i32 @a() {'; echo '  ret i32 0'; echo '}';
  echo ' define not'; echo 'define void @b() {'; echo '}'; cat "$src"; } > "$out""#;

    fn input(id: &str, source: &str) -> Input {
        Input {
            id: id.to_owned(),
            class: Class::Small,
            debug: false,
            source: source.to_owned(),
            lang: Lang::of(source).unwrap(),
            args: vec!["-O0".to_owned(), "-S".to_owned()],
        }
    }

    // "Inputs": an input is produced in bench/src, described by its size,
    // function count and hash, and made again only when its compiler, its
    // arguments or its source change, or when it is forced.
    #[test]
    fn inputs_are_generated_and_cached() {
        let scratch = Scratch::new("bench-generate");
        let cxx = scratch.write_script("clang++-22", FAKE_COMPILER);
        let src = scratch.join("src");
        let out = scratch.join("out/clang22");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.cpp"), "; the source\n").unwrap();
        let producer = Producer {
            tag: "clang22".into(),
            c: None,
            cxx: Some(cxx.clone()),
            version: "fake clang version 22.0".into(),
        };
        let runs = || {
            std::fs::read_to_string(scratch.join("clang++-22.runs"))
                .map_or(0, |t| t.lines().count())
        };
        let make = |input: &Input, producer: &Producer, force: bool| {
            generate(input, producer, &src, &out, force, PRODUCER_LIMITS)
        };

        let a = input("a", "a.cpp");
        let first = make(&a, &producer, false).unwrap();
        let text = std::fs::read_to_string(&first.path).unwrap();
        let cwd = src.canonicalize().unwrap();
        assert_eq!(first.path, out.join("a.ll"));
        assert!(text.starts_with(&format!("; args -O0 -S\n; cwd {}\n", cwd.display())), "{text}");
        assert_eq!((first.ir_bytes, first.functions), (text.len() as u64, 2));
        assert_eq!(first.ir_hash, fnv1a64(text.as_bytes()));
        assert!(first.fresh);
        assert_eq!(
            describe(b"define a\n define b\ndefine c"),
            (27, 2, fnv1a64(b"define a\n define b\ndefine c"))
        );

        // Up to date: the compiler is not run again.
        let again = make(&a, &producer, false).unwrap();
        assert_eq!(again, Generated { fresh: false, ..first.clone() });
        assert_eq!(runs(), 1);
        // Forced; other arguments; another compiler version; another
        // source; a file that is not the one the stamp describes.
        assert!(make(&a, &producer, true).unwrap().fresh);
        let other_args = Input { args: vec!["-O0".to_owned()], ..a.clone() };
        assert!(make(&other_args, &producer, false).unwrap().fresh);
        assert!(make(&a, &producer, false).unwrap().fresh);
        let newer = Producer { version: "fake clang version 22.1".into(), ..producer.clone() };
        assert!(make(&a, &newer, false).unwrap().fresh);
        std::fs::write(src.join("a.cpp"), "; the source, edited\n").unwrap();
        let edited = make(&a, &newer, false).unwrap();
        assert!(edited.fresh && edited.ir_hash != first.ir_hash);
        std::fs::write(out.join("a.ll"), "truncated").unwrap();
        assert!(make(&a, &newer, false).unwrap().fresh);
        assert_eq!(runs(), 7);
        assert!(!make(&a, &newer, false).unwrap().fresh);

        // Nothing but the inputs and their stamps is left behind.
        let mut names: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["a.ll", "a.stamp"]);
    }

    // "Inputs": a compile that fails, writes nothing or hangs is an error
    // that says so, and leaves no input behind.
    #[test]
    fn generation_failures() {
        let scratch = Scratch::new("bench-generate-fail");
        let cxx = scratch.write_script("clang++-22", FAKE_COMPILER);
        let src = scratch.join("src");
        let out = scratch.join("out");
        std::fs::create_dir_all(&src).unwrap();
        let producer =
            Producer { tag: "clang22".into(), c: None, cxx: Some(cxx), version: "v".into() };
        let make = |name: &str, text: &str, limits: Limits| {
            let file = format!("{name}.cpp");
            std::fs::write(src.join(&file), text).unwrap();
            generate(&input(name, &file), &producer, &src, &out, false, limits)
        };
        assert_eq!(
            make("bad", "fail\n", PRODUCER_LIMITS).unwrap_err(),
            "the compiler exited with status 1: fake: error: cannot compile bad.cpp"
        );
        assert_eq!(
            make("quiet", "silent\n", PRODUCER_LIMITS).unwrap_err(),
            "the compiler wrote no IR"
        );
        let short = Limits { time: Duration::from_millis(300), ..PRODUCER_LIMITS };
        assert_eq!(make("slow", "hang\n", short).unwrap_err(), "the compiler timed out after 0 s");
        assert!(!out.join("bad.ll").exists() && !out.join("bad.stamp").exists());
        let c_source = input("c", "c.c");
        assert_eq!(
            generate(&c_source, &producer, &src, &out, false, PRODUCER_LIMITS).unwrap_err(),
            "the producer (clang22) has no C compiler"
        );
        let missing = input("missing", "missing.cpp");
        assert!(
            generate(&missing, &producer, &src, &out, false, PRODUCER_LIMITS)
                .unwrap_err()
                .starts_with("cannot read '")
        );
    }

    // "Inputs": the compilers are clang of the oracle's LLVM, found beside
    // it; or kiln++ and the kiln beside it.
    #[test]
    fn producers_are_found() {
        let scratch = Scratch::new("bench-producer");
        for name in [
            "bin/llc-22",
            "bin/clang-22",
            "bin/clang++-22",
            "other/clang++-19",
            "root/llvm-19/bin/clang",
        ] {
            std::fs::create_dir_all(scratch.join(name).parent().unwrap()).unwrap();
            hod_testutil::write_script(&scratch.join(name), FAKE_COMPILER);
        }
        let search = Search {
            path: std::env::join_paths([scratch.join("other")]).unwrap(),
            llvm_root: scratch.join("root"),
        };
        let llvm22 = Llvm { llc: scratch.join("bin/llc-22"), major: 22 };
        let producer = Producer::clang(&llvm22, &search).unwrap();
        assert_eq!(producer.tag, "clang22");
        assert_eq!(producer.c, Some(scratch.join("bin/clang-22")));
        assert_eq!(producer.cxx, Some(scratch.join("bin/clang++-22")));
        assert_eq!(producer.version, "fake clang version 22.0");

        // An installation directory has plain names; here there is none
        // beside llc, so PATH and the LLVM root are searched.
        let llvm19 = Llvm { llc: scratch.join("root/llvm-19/bin/llc"), major: 19 };
        let producer = Producer::clang(&llvm19, &search).unwrap();
        assert_eq!(producer.tag, "clang19");
        assert_eq!(producer.c, Some(scratch.join("root/llvm-19/bin/clang")));
        assert_eq!(producer.cxx, Some(scratch.join("other/clang++-19")));
        let llvm20 = Llvm { llc: scratch.join("bin/llc-20"), major: 20 };
        assert_eq!(
            Producer::clang(&llvm20, &search).unwrap_err(),
            "no clang of LLVM 20 found to produce the inputs"
        );

        let kilnxx = scratch.write_script("kiln++", FAKE_COMPILER);
        let alone = Producer::kiln(&kilnxx).unwrap();
        assert_eq!(
            (alone.tag.as_str(), &alone.c, &alone.cxx),
            ("kiln", &None, &Some(kilnxx.clone()))
        );
        let kiln = scratch.write_script("kiln", FAKE_COMPILER);
        assert_eq!(Producer::kiln(&kilnxx).unwrap().c, Some(kiln));
        assert!(Producer::kiln(&scratch.join("absent")).unwrap_err().ends_with("is not a file"));
    }
}
